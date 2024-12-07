use crate::ipc::IpcMethod;
use crate::Config;
use crate::{app::App, ipc::UserWindowEvent};
use dioxus_core::*;
use dioxus_document::eval;
use std::any::Any;
use std::rc::Rc;
use winit::application::ApplicationHandler;
use winit::event_loop::{self, ActiveEventLoop, EventLoop};

/// Launch the WebView and run the event loop, with configuration and root props.
///
/// This will block the main thread, and *must* be spawned on the main thread. This function does not assume any runtime
/// and is equivalent to calling launch_with_props with the tokio feature disabled.
pub fn launch_virtual_dom_blocking(virtual_dom: VirtualDom, mut desktop_config: Config) {
    let custom_event_handler = desktop_config.custom_event_handler.take();
    let (event_loop, app) = App::new(desktop_config, virtual_dom);

    event_loop.run_app(&mut Launch {
        app,
        custom_event_handler,
    });
}

struct Launch {
    app: App,
    custom_event_handler: Option<
        Box<dyn FnMut(&winit::event::Event<UserWindowEvent>, &winit::event_loop::ActiveEventLoop)>,
    >,
}

impl ApplicationHandler<UserWindowEvent> for Launch {
    fn new_events(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        cause: winit::event::StartCause,
    ) {
        self.app.handle_start_cause_init();

        *control_flow = self.app.control_flow;
    }

    fn user_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        event: UserWindowEvent,
    ) {
        if let Some(ref mut f) = self.custom_event_handler {
            f(&event, &event_loop)
        }
        match event {
            UserWindowEvent::Poll(id) => self.app.poll_vdom(id),
            UserWindowEvent::NewWindow => self.app.handle_new_window(),
            UserWindowEvent::CloseWindow(id) => self.app.handle_close_msg(id),
            UserWindowEvent::Shutdown => event_loop.exit(),

            #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
            UserWindowEvent::GlobalHotKeyEvent(evnt) => self.app.handle_global_hotkey(evnt),

            #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
            UserWindowEvent::MudaMenuEvent(evnt) => self.app.handle_menu_event(evnt),

            #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
            UserWindowEvent::TrayMenuEvent(evnt) => self.app.handle_tray_menu_event(evnt),

            #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
            UserWindowEvent::TrayIconEvent(evnt) => self.app.handle_tray_icon_event(evnt),

            #[cfg(all(feature = "devtools", debug_assertions))]
            UserWindowEvent::HotReloadEvent(msg) => self.app.handle_hot_reload_msg(msg),

            // Windows-only drag-n-drop fix events. We need to call the interpreter drag-n-drop code.
            UserWindowEvent::WindowsDragDrop(id) => {
                if let Some(webview) = self.app.webviews.get(&id) {
                    webview.dom.in_runtime(|| {
                        ScopeId::ROOT.in_runtime(|| {
                            eval("window.interpreter.handleWindowsDragDrop();");
                        });
                    });
                }
            }
            UserWindowEvent::WindowsDragLeave(id) => {
                if let Some(webview) = self.app.webviews.get(&id) {
                    webview.dom.in_runtime(|| {
                        ScopeId::ROOT.in_runtime(|| {
                            eval("window.interpreter.handleWindowsDragLeave();");
                        });
                    });
                }
            }
            UserWindowEvent::WindowsDragOver(id, x_pos, y_pos) => {
                if let Some(webview) = self.app.webviews.get(&id) {
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
                IpcMethod::Initialize => self.app.handle_initialize_msg(id),
                IpcMethod::FileDialog => self.app.handle_file_dialog_msg(msg, id),
                IpcMethod::UserEvent => {}
                IpcMethod::Query => self.app.handle_query_msg(msg, id),
                IpcMethod::BrowserOpen => self.app.handle_browser_open(msg),
                IpcMethod::Other(_) => {}
            },
        }
    }

    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        self.app.handle_start_cause_init();
        self.app.shared.target = Some(event_loop);
    }

    fn window_event(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        self.app.tick(&event);
    }
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
        .expect("unable to create config");

    launch_virtual_dom(virtual_dom, platform_config)
}
