use crate::Config;
use crate::{
    app::App,
    ipc::{IpcMethod, UserWindowEvent},
};
use dioxus_core::*;
use dioxus_document::eval;
use std::any::Any;
use std::fmt::Debug;
use std::result::Result::Ok;
use std::sync::mpsc::SyncSender;
use std::sync::Mutex;
use std::sync::{mpsc::Receiver, Arc};
use tao::event::{self, Event, StartCause, WindowEvent};
use tao::event_loop::ControlFlow;
use tao::window::{Window, WindowBuilder};

/// Launch the WebView and run the event loop, with configuration and root props.
///
/// This will block the main thread, and *must* be spawned on the main thread. This function does not assume any runtime
/// and is equivalent to calling launch_with_props with the tokio feature disabled.
pub fn launch_virtual_dom_blocking(virtual_dom: VirtualDom, mut desktop_config: Config) -> ! {
    let mut custom_event_handler = desktop_config.custom_event_handler.take();
    let (event_loop, mut app) = App::new(desktop_config, virtual_dom);

    event_loop.run(move |window_event, event_loop, control_flow| {
        // Set the control flow and check if any events need to be handled in the app itself
        app.tick(&window_event);

        if let Some(ref mut f) = custom_event_handler {
            f(&window_event, event_loop)
        }

        match window_event {
            Event::NewEvents(StartCause::Init) => app.handle_start_cause_init(),
            Event::LoopDestroyed => app.handle_loop_destroyed(),
            Event::WindowEvent {
                event, window_id, ..
            } => match event {
                WindowEvent::CloseRequested => app.handle_close_requested(window_id),
                WindowEvent::Destroyed { .. } => app.window_destroyed(window_id),
                WindowEvent::Resized(new_size) => app.resize_window(new_size),
                _ => {}
            },

            Event::UserEvent(event) => match event {
                UserWindowEvent::Poll(id) => app.poll_vdom(id),
                UserWindowEvent::NewWindow => app.handle_new_window(),
                UserWindowEvent::CloseWindow(id) => app.handle_close_msg(id),
                UserWindowEvent::Shutdown => app.control_flow = tao::event_loop::ControlFlow::Exit,

                #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
                UserWindowEvent::GlobalHotKeyEvent(evnt) => app.handle_global_hotkey(evnt),

                #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
                UserWindowEvent::MudaMenuEvent(evnt) => app.handle_menu_event(evnt),

                #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
                UserWindowEvent::TrayMenuEvent(evnt) => app.handle_tray_menu_event(evnt),

                #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
                UserWindowEvent::TrayIconEvent(evnt) => app.handle_tray_icon_event(evnt),

                #[cfg(all(feature = "devtools", debug_assertions))]
                UserWindowEvent::HotReloadEvent(msg) => app.handle_hot_reload_msg(msg),

                // Windows-only drag-n-drop fix events. We need to call the interpreter drag-n-drop code.
                UserWindowEvent::WindowsDragDrop(id) => {
                    if let Some(webview) = app.webviews.get(&id) {
                        webview.dom.in_runtime(|| {
                            ScopeId::ROOT.in_runtime(|| {
                                eval("window.interpreter.handleWindowsDragDrop();");
                            });
                        });
                    }
                }
                UserWindowEvent::WindowsDragLeave(id) => {
                    if let Some(webview) = app.webviews.get(&id) {
                        webview.dom.in_runtime(|| {
                            ScopeId::ROOT.in_runtime(|| {
                                eval("window.interpreter.handleWindowsDragLeave();");
                            });
                        });
                    }
                }
                UserWindowEvent::WindowsDragOver(id, x_pos, y_pos) => {
                    if let Some(webview) = app.webviews.get(&id) {
                        webview.dom.in_runtime(|| {
                            ScopeId::ROOT.in_runtime(|| {
                                let e = eval(
                                    r#"
                                    const xPos = await dioxus.recv();
                                    const yPos = await dioxus.recv();
                                    window.interpreter.handleWindowsDragOver(xPos, yPos)
                                    "#,
                                );

                                _ = e.send(x_pos);
                                _ = e.send(y_pos);
                            });
                        });
                    }
                }

                UserWindowEvent::Ipc { id, msg } => match msg.method() {
                    IpcMethod::Initialize => app.handle_initialize_msg(id),
                    IpcMethod::FileDialog => app.handle_file_dialog_msg(msg, id),
                    IpcMethod::UserEvent => {}
                    IpcMethod::Query => app.handle_query_msg(msg, id),
                    IpcMethod::BrowserOpen => app.handle_browser_open(msg),
                    IpcMethod::Other(_) => {}
                },
            },
            _ => {}
        }

        *control_flow = app.control_flow;
    })
}

/// App channels
#[derive(Debug, Clone)]
pub struct AppChannels {
    /// Sender for sending messages to the WebViewx
    pub tx: SyncSender<String>,
    /// Receiver for receiving messages from the WebView
    pub rx: Arc<Mutex<Receiver<String>>>,
}
/// Launch the WebView and run the event loop, with configuration and root props.
pub fn launch_virtual_dom_blockin_with_custom_window<
    T: 'static,
    P: 'static + Send + Debug + Clone,
>(
    virtual_dom: VirtualDom,
    mut desktop_config: Config,
    window_builder: WindowBuilder,
    mut app_custom: T,
    set_window_state: impl FnOnce(&mut T, Arc<Window>) + 'static + Clone,
    custom_runner: fn(&mut T, &event::Event<'_, UserWindowEvent>, &mut ControlFlow),
    props: P,
) -> !
where
    P: Into<AppChannels>,
{
    let props: AppChannels = props.into();

    let mut custom_event_handler = desktop_config.custom_event_handler.take();
    let (event_loop, mut app) = App::new(desktop_config, virtual_dom);

    let custom_window: Window = window_builder.build(&event_loop).unwrap();
    let custom_window = Arc::new(custom_window);

    set_window_state(&mut app_custom, custom_window.clone());

    event_loop.run(move |window_event, event_loop, control_flow| {
        app.tick(&window_event);

        if let Ok(message) = props.rx.lock() {
            if let Ok(message) = message.try_recv() {
                if message == "init" {
                    custom_window.set_visible(true);
                    custom_runner(&mut app_custom, &window_event, control_flow);
                    if let Some(ref mut handler) = custom_event_handler {
                        handler(&window_event, event_loop);
                    }
                }
            }
        }

        if custom_window.is_visible() {
            custom_runner(&mut app_custom, &window_event, control_flow);

            if let Some(ref mut handler) = custom_event_handler {
                handler(&window_event, event_loop);
            }
        }

        match window_event {
            Event::NewEvents(StartCause::Init) => app.handle_start_cause_init(),
            Event::LoopDestroyed => app.handle_loop_destroyed(),
            Event::WindowEvent {
                event, window_id, ..
            } => match event {
                WindowEvent::CloseRequested => app.handle_close_requested(window_id),
                WindowEvent::Destroyed { .. } => app.window_destroyed(window_id),
                WindowEvent::Resized(new_size) => app.resize_window(new_size),
                _ => {}
            },

            Event::UserEvent(event) => match event {
                UserWindowEvent::Poll(id) => app.poll_vdom(id),
                UserWindowEvent::NewWindow => app.handle_new_window(),
                UserWindowEvent::CloseWindow(id) => app.handle_close_msg(id),
                UserWindowEvent::Shutdown => app.control_flow = tao::event_loop::ControlFlow::Exit,

                #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
                UserWindowEvent::GlobalHotKeyEvent(evnt) => app.handle_global_hotkey(evnt),

                #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
                UserWindowEvent::MudaMenuEvent(evnt) => app.handle_menu_event(evnt),

                #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
                UserWindowEvent::TrayMenuEvent(evnt) => app.handle_tray_menu_event(evnt),

                #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
                UserWindowEvent::TrayIconEvent(evnt) => app.handle_tray_icon_event(evnt),

                #[cfg(all(feature = "devtools", debug_assertions))]
                UserWindowEvent::HotReloadEvent(msg) => app.handle_hot_reload_msg(msg),

                // Windows-only drag-n-drop fix events
                UserWindowEvent::WindowsDragDrop(id) => {
                    if let Some(webview) = app.webviews.get(&id) {
                        webview.dom.in_runtime(|| {
                            ScopeId::ROOT.in_runtime(|| {
                                eval("window.interpreter.handleWindowsDragDrop();");
                            });
                        });
                    }
                }
                UserWindowEvent::WindowsDragLeave(id) => {
                    if let Some(webview) = app.webviews.get(&id) {
                        webview.dom.in_runtime(|| {
                            ScopeId::ROOT.in_runtime(|| {
                                eval("window.interpreter.handleWindowsDragLeave();");
                            });
                        });
                    }
                }
                UserWindowEvent::WindowsDragOver(id, x_pos, y_pos) => {
                    if let Some(webview) = app.webviews.get(&id) {
                        webview.dom.in_runtime(|| {
                            ScopeId::ROOT.in_runtime(|| {
                                let e = eval(
                                    r#"
                                    const xPos = await dioxus.recv();
                                    const yPos = await dioxus.recv();
                                    window.interpreter.handleWindowsDragOver(xPos, yPos)
                                    "#,
                                );

                                _ = e.send(x_pos);
                                _ = e.send(y_pos);
                            });
                        });
                    }
                }

                UserWindowEvent::Ipc { id, msg } => match msg.method() {
                    IpcMethod::Initialize => app.handle_initialize_msg(id),
                    IpcMethod::FileDialog => app.handle_file_dialog_msg(msg, id),
                    IpcMethod::UserEvent => {}
                    IpcMethod::Query => app.handle_query_msg(msg, id),
                    IpcMethod::BrowserOpen => app.handle_browser_open(msg),
                    IpcMethod::Other(_) => {}
                },
            },
            _ => {}
        }

        // Update the control flow
        *control_flow = app.control_flow;
    })
}

/// Launch a window with custom configuration and event handling.
pub fn launch_with_custom_window<T: 'static + Send, P: 'static + Send + Debug + Clone>(
    root: fn(P) -> Element,
    contexts: Vec<Box<dyn Fn() -> Box<dyn Any> + Send + Sync>>,
    platform_config: Config,
    window_builder: WindowBuilder,
    app_custom: T,
    set_window_state: fn(&mut T, Arc<Window>),
    custom_runner: fn(&mut T, &event::Event<'_, UserWindowEvent>, &mut ControlFlow),
    props: P,
) -> !
where
    P: Into<AppChannels>,
{
    let mut virtual_dom = VirtualDom::new_with_props(root, props.clone());

    for context in contexts {
        virtual_dom.insert_any_root_context(context())
    }

    launch_virtual_dom_blockin_with_custom_window(
        virtual_dom,
        platform_config,
        window_builder,
        app_custom,
        set_window_state,
        custom_runner,
        props,
    );
}

/// Launches the WebView and runs the event loop, with configuration and root props.
pub fn launch_virtual_dom(virtual_dom: VirtualDom, desktop_config: Config) -> ! {
    #[cfg(feature = "tokio_runtime")]
    {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(tokio::task::unconstrained(async move {
                launch_virtual_dom_blocking(virtual_dom, desktop_config)
            }));

        unreachable!("The desktop launch function will never exit")
    }

    #[cfg(not(feature = "tokio_runtime"))]
    {
        launch_virtual_dom_blocking(virtual_dom, desktop_config);
    }
}

/// Launches the WebView and runs the event loop, with configuration and root props.
pub fn launch(
    root: fn() -> Element,
    contexts: Vec<Box<dyn Fn() -> Box<dyn Any> + Send + Sync>>,
    platform_config: Vec<Box<dyn Any>>,
) -> ! {
    let mut virtual_dom = VirtualDom::new(root);

    for context in contexts {
        virtual_dom.insert_any_root_context(context());
    }

    let platform_config = *platform_config
        .into_iter()
        .find_map(|cfg| cfg.downcast::<Config>().ok())
        .unwrap_or_default();

    launch_virtual_dom(virtual_dom, platform_config)
}
