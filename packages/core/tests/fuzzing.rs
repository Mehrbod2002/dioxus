use dioxus::prelude::Props;
use dioxus_core::*;
use std::cell::Cell;

fn random_ns() -> Option<&'static str> {
    let namespace = rand::random::<u8>() % 2;
    match namespace {
        0 => None,
        1 => Some(Box::leak(
            format!("ns{}", rand::random::<usize>()).into_boxed_str(),
        )),
        _ => unreachable!(),
    }
}

fn create_random_attribute(attr_idx: &mut usize) -> TemplateAttribute<'static> {
    match rand::random::<u8>() % 2 {
        0 => TemplateAttribute::Static {
            name: Box::leak(format!("attr{}", rand::random::<usize>()).into_boxed_str()),
            value: Box::leak(format!("value{}", rand::random::<usize>()).into_boxed_str()),
            namespace: random_ns(),
        },
        1 => TemplateAttribute::Dynamic {
            id: {
                let old_idx = *attr_idx;
                *attr_idx += 1;
                old_idx
            },
        },
        _ => unreachable!(),
    }
}

fn create_random_template_node(
    template_idx: &mut usize,
    attr_idx: &mut usize,
    depth: usize,
) -> TemplateNode<'static> {
    match rand::random::<u8>() % 4 {
        0 => {
            let attrs = {
                let attrs: Vec<_> = (0..(rand::random::<usize>() % 10))
                    .map(|_| create_random_attribute(attr_idx))
                    .collect();
                Box::leak(attrs.into_boxed_slice())
            };
            TemplateNode::Element {
                tag: Box::leak(format!("tag{}", rand::random::<usize>()).into_boxed_str()),
                namespace: random_ns(),
                attrs,
                children: {
                    if depth > 10 {
                        &[]
                    } else {
                        let children: Vec<_> = (0..(rand::random::<usize>() % 3))
                            .map(|_| create_random_template_node(template_idx, attr_idx, depth + 1))
                            .collect();
                        Box::leak(children.into_boxed_slice())
                    }
                },
            }
        }
        1 => TemplateNode::Text {
            text: Box::leak(format!("{}", rand::random::<usize>()).into_boxed_str()),
        },
        2 => TemplateNode::DynamicText {
            id: {
                let old_idx = *template_idx;
                *template_idx += 1;
                old_idx
            },
        },
        3 => TemplateNode::Dynamic {
            id: {
                let old_idx = *template_idx;
                *template_idx += 1;
                old_idx
            },
        },
        _ => unreachable!(),
    }
}

fn generate_paths(
    node: &TemplateNode<'static>,
    current_path: &Vec<u8>,
    node_paths: &mut Vec<Vec<u8>>,
    attr_paths: &mut Vec<Vec<u8>>,
) {
    match node {
        TemplateNode::Element { children, attrs, .. } => {
            for attr in *attrs {
                match attr {
                    TemplateAttribute::Static { .. } => {}
                    TemplateAttribute::Dynamic { .. } => {
                        attr_paths.push(current_path.clone());
                    }
                }
            }
            for (i, child) in children.iter().enumerate() {
                let mut current_path = current_path.clone();
                current_path.push(i as u8);
                generate_paths(child, &current_path, node_paths, attr_paths);
            }
        }
        TemplateNode::Text { .. } => {}
        TemplateNode::DynamicText { .. } => {
            node_paths.push(current_path.clone());
        }
        TemplateNode::Dynamic { .. } => {
            node_paths.push(current_path.clone());
        }
    }
}

fn create_random_template(name: &'static str) -> Template<'static> {
    let mut template_idx = 0;
    let mut attr_idx = 0;
    let roots = (0..(1 + rand::random::<usize>() % 5))
        .map(|_| create_random_template_node(&mut template_idx, &mut attr_idx, 0))
        .collect::<Vec<_>>();
    assert!(roots.len() > 0);
    let roots = Box::leak(roots.into_boxed_slice());
    let mut node_paths = Vec::new();
    let mut attr_paths = Vec::new();
    for (i, root) in roots.iter().enumerate() {
        generate_paths(root, &vec![i as u8], &mut node_paths, &mut attr_paths);
    }
    let node_paths = Box::leak(
        node_paths
            .into_iter()
            .map(|v| &*Box::leak(v.into_boxed_slice()))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    let attr_paths = Box::leak(
        attr_paths
            .into_iter()
            .map(|v| &*Box::leak(v.into_boxed_slice()))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    Template { name, roots, node_paths, attr_paths }
}

fn create_random_dynamic_node<'a>(cx: &'a ScopeState, depth: usize) -> DynamicNode<'a> {
    let range = if depth > 10 { 2 } else { 4 };
    match rand::random::<u8>() % range {
        0 => DynamicNode::Text(VText {
            value: Box::leak(format!("{}", rand::random::<usize>()).into_boxed_str()),
            id: Default::default(),
        }),
        1 => DynamicNode::Placeholder(Default::default()),
        2 => cx.make_node((0..(rand::random::<u8>() % 5)).map(|_| VNode {
            key: None,
            parent: Default::default(),
            template: Cell::new(Template {
                name: concat!(file!(), ":", line!(), ":", column!(), ":0"),
                roots: &[TemplateNode::Dynamic { id: 0 }],
                node_paths: &[&[0]],
                attr_paths: &[],
            }),
            root_ids: Default::default(),
            dynamic_nodes: cx.bump().alloc([cx.component(
                create_random_element,
                DepthProps { depth, root: false },
                "create_random_element",
            )]),
            dynamic_attrs: &[],
        })),
        3 => cx.component(
            create_random_element,
            DepthProps { depth, root: false },
            "create_random_element",
        ),
        _ => unreachable!(),
    }
}

fn create_random_dynamic_attr<'a>(cx: &'a ScopeState) -> Attribute<'a> {
    let value = match rand::random::<u8>() % 6 {
        0 => AttributeValue::Text(Box::leak(
            format!("{}", rand::random::<usize>()).into_boxed_str(),
        )),
        1 => AttributeValue::Float(rand::random()),
        2 => AttributeValue::Int(rand::random()),
        3 => AttributeValue::Bool(rand::random()),
        4 => cx.any_value(rand::random::<usize>()),
        5 => AttributeValue::None,
        // Listener(RefCell<Option<ListenerCb<'a>>>),
        _ => unreachable!(),
    };
    Attribute {
        name: Box::leak(format!("attr{}", rand::random::<usize>()).into_boxed_str()),
        value,
        namespace: random_ns(),
        mounted_element: Default::default(),
        volatile: rand::random(),
    }
}

static mut TEMPLATE_COUNT: usize = 0;

#[derive(PartialEq, Props)]
struct DepthProps {
    depth: usize,
    root: bool,
}

fn create_random_element<'a>(cx: Scope<'a, DepthProps>) -> Element<'a> {
    cx.needs_update();
    let range = if cx.props.root { 2 } else { 3 };
    let node = match rand::random::<usize>() % range {
        0 | 1 => {
            let template = create_random_template(Box::leak(
                format!(
                    "{}{}",
                    concat!(file!(), ":", line!(), ":", column!(), ":"),
                    {
                        unsafe {
                            let old = TEMPLATE_COUNT;
                            TEMPLATE_COUNT += 1;
                            old
                        }
                    }
                )
                .into_boxed_str(),
            ));
            println!("{:#?}", template);
            let node = VNode {
                key: None,
                parent: None,
                template: Cell::new(template),
                root_ids: Default::default(),
                dynamic_nodes: {
                    let dynamic_nodes: Vec<_> = (0..template.node_paths.len())
                        .map(|_| create_random_dynamic_node(cx, cx.props.depth + 1))
                        .collect();
                    cx.bump().alloc(dynamic_nodes)
                },
                dynamic_attrs: cx.bump().alloc(
                    (0..template.attr_paths.len())
                        .map(|_| create_random_dynamic_attr(cx))
                        .collect::<Vec<_>>(),
                ),
            };
            Some(node)
        }
        _ => None,
    };
    println!("{:#?}", node);
    node
}

// test for panics when creating random nodes and templates
#[test]
fn create() {
    for _ in 0..1000 {
        let mut vdom =
            VirtualDom::new_with_props(create_random_element, DepthProps { depth: 0, root: true });
        let _ = vdom.rebuild();
    }
}

// test for panics when diffing random nodes
// This test will change the template every render which is not very realistic, but it helps stress the system
#[test]
fn diff() {
    for _ in 0..100 {
        let mut vdom =
            VirtualDom::new_with_props(create_random_element, DepthProps { depth: 0, root: true });
        let _ = vdom.rebuild();
        for _ in 0..10 {
            let _ = vdom.render_immediate();
        }
    }
}
