//! Parse the root tokens in the rsx!{} macro
//! =========================================
//!
//! This parsing path emerges directly from the macro call, with `RsxRender` being the primary entrance into parsing.
//! This feature must support:
//! - [x] Optionally rendering if the `in XYZ` pattern is present
//! - [x] Fragments as top-level element (through ambiguous)
//! - [x] Components as top-level element (through ambiguous)
//! - [x] Tags as top-level elements (through ambiguous)
//! - [x] Good errors if parsing fails
//!
//! Any errors in using rsx! will likely occur when people start using it, so the first errors must be really helpful.

#[macro_use]
mod errors;
mod component;
mod element;
mod ifmt;
mod node;

// Re-export the namespaces into each other
pub use component::*;
use dioxus_core::{Template, TemplateAttribute, TemplateNode};
pub use element::*;
pub use ifmt::*;
pub use node::*;

// imports
use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, ToTokens, TokenStreamExt};
use syn::{
    parse::{Parse, ParseStream},
    Result, Token,
};

/// Fundametnally, every CallBody is a template
#[derive(Default)]
pub struct CallBody {
    pub roots: Vec<BodyNode>,

    // set this after
    pub inline_cx: bool,
}

impl CallBody {
    /// This function intentionally leaks memory to create a static template.
    /// Keeping the template static allows us to simplify the core of dioxus and leaking memory in dev mode is less of an issue.
    /// the previous_location is the location of the previous template at the time the template was originally compiled.
    pub fn leak_template(&self, previous_location: &'static str) -> Template {
        let mut renderer = TemplateRenderer { roots: &self.roots };
        renderer.leak_template(previous_location)
    }
}

impl Parse for CallBody {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut roots = Vec::new();

        while !input.is_empty() {
            let node = input.parse::<BodyNode>()?;

            if input.peek(Token![,]) {
                let _ = input.parse::<Token![,]>();
            }

            roots.push(node);
        }

        Ok(Self {
            roots,
            inline_cx: false,
        })
    }
}

/// Serialize the same way, regardless of flavor
impl ToTokens for CallBody {
    fn to_tokens(&self, out_tokens: &mut TokenStream2) {
        let body = TemplateRenderer { roots: &self.roots };

        if self.inline_cx {
            out_tokens.append_all(quote! {
                Ok({
                    let __cx = cx;
                    #body
                })
            })
        } else {
            out_tokens.append_all(quote! {
                ::dioxus::core::LazyNodes::new( move | __cx: &::dioxus::core::ScopeState| -> ::dioxus::core::VNode {
                    #body
                })
            })
        }
    }
}

pub struct TemplateRenderer<'a> {
    pub roots: &'a [BodyNode],
}

impl<'a> TemplateRenderer<'a> {
    fn leak_template(&mut self, previous_location: &'static str) -> Template<'static> {
        let mut context = DynamicContext::default();

        let roots: Vec<_> = self
            .roots
            .iter()
            .enumerate()
            .map(|(idx, root)| {
                context.current_path.push(idx as u8);
                let out = context.leak_node(root);
                context.current_path.pop();
                out
            })
            .collect();

        Template {
            name: previous_location,
            roots: Box::leak(roots.into_boxed_slice()),
            node_paths: Box::leak(
                context
                    .node_paths
                    .into_iter()
                    .map(|path| &*Box::leak(path.into_boxed_slice()))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
            attr_paths: Box::leak(
                context
                    .attr_paths
                    .into_iter()
                    .map(|path| &*Box::leak(path.into_boxed_slice()))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
        }
    }
}

impl<'a> ToTokens for TemplateRenderer<'a> {
    fn to_tokens(&self, out_tokens: &mut TokenStream2) {
        let mut context = DynamicContext::default();

        let key = match self.roots.get(0) {
            Some(BodyNode::Element(el)) if self.roots.len() == 1 => el.key.clone(),
            Some(BodyNode::Component(comp)) if self.roots.len() == 1 => comp.key().cloned(),
            _ => None,
        };

        let key_tokens = match key {
            Some(tok) => quote! { Some( __cx.raw_text(#tok) ) },
            None => quote! { None },
        };

        let spndbg = format!("{:?}", self.roots[0].span());
        let root_col = spndbg[9..].split("..").next().unwrap();

        let root_printer = self.roots.iter().enumerate().map(|(idx, root)| {
            context.current_path.push(idx as u8);
            let out = context.render_static_node(root);
            context.current_path.pop();
            out
        });

        // Render and release the mutable borrow on context
        let num_roots = self.roots.len();
        let roots = quote! { #( #root_printer ),* };
        let node_printer = &context.dynamic_nodes;
        let dyn_attr_printer = &context.dynamic_attributes;
        let node_paths = context.node_paths.iter().map(|it| quote!(&[#(#it),*]));
        let attr_paths = context.attr_paths.iter().map(|it| quote!(&[#(#it),*]));

        out_tokens.append_all(quote! {
            static TEMPLATE: ::dioxus::core::Template = ::dioxus::core::Template {
                name: concat!(
                    file!(),
                    ":",
                    line!(),
                    ":",
                    column!(),
                    ":",
                    #root_col
                ),
                roots: &[ #roots ],
                node_paths: &[ #(#node_paths),* ],
                attr_paths: &[ #(#attr_paths),* ],
            };
            ::dioxus::core::VNode {
                parent: None,
                key: #key_tokens,
                template: TEMPLATE,
                root_ids: std::cell::Cell::from_mut( __cx.bump().alloc([::dioxus::core::ElementId(0); #num_roots]) as &mut [::dioxus::core::ElementId]).as_slice_of_cells(),
                dynamic_nodes: __cx.bump().alloc([ #( #node_printer ),* ]),
                dynamic_attrs: __cx.bump().alloc([ #( #dyn_attr_printer ),* ]),
            }
        });
    }
}
// As we print out the dynamic nodes, we want to keep track of them in a linear fashion
// We'll use the size of the vecs to determine the index of the dynamic node in the final
#[derive(Default)]
pub struct DynamicContext<'a> {
    dynamic_nodes: Vec<&'a BodyNode>,
    dynamic_attributes: Vec<&'a ElementAttrNamed>,
    current_path: Vec<u8>,

    node_paths: Vec<Vec<u8>>,
    attr_paths: Vec<Vec<u8>>,
}

impl<'a> DynamicContext<'a> {
    fn leak_node(&mut self, root: &'a BodyNode) -> TemplateNode<'static> {
        match root {
            BodyNode::Element(el) => {
                // dynamic attributes
                // [0]
                // [0, 1]
                // [0, 1]
                // [0, 1]
                // [0, 1, 2]
                // [0, 2]
                // [0, 2, 1]

                let static_attrs: Vec<_> = el
                    .attributes
                    .iter()
                    .map(|attr| match &attr.attr {
                        ElementAttr::AttrText { name: _, value } if value.is_static() => {
                            let value = value.source.as_ref().unwrap();
                            TemplateAttribute::Static {
                                name: "todo",
                                namespace: None,
                                value: Box::leak(value.value().into_boxed_str()),
                                // name: dioxus_elements::#el_name::#name.0,
                                // namespace: dioxus_elements::#el_name::#name.1,
                                // value: #value,

                                // todo: we don't diff these so we never apply the volatile flag
                                // volatile: dioxus_elements::#el_name::#name.2,
                            }
                        }

                        ElementAttr::CustomAttrText { name, value } if value.is_static() => {
                            let value = value.source.as_ref().unwrap();
                            TemplateAttribute::Static {
                                name: Box::leak(name.value().into_boxed_str()),
                                namespace: None,
                                value: Box::leak(value.value().into_boxed_str()),
                                // todo: we don't diff these so we never apply the volatile flag
                                // volatile: dioxus_elements::#el_name::#name.2,
                            }
                        }

                        ElementAttr::AttrExpression { .. }
                        | ElementAttr::AttrText { .. }
                        | ElementAttr::CustomAttrText { .. }
                        | ElementAttr::CustomAttrExpression { .. }
                        | ElementAttr::EventTokens { .. } => {
                            let ct = self.dynamic_attributes.len();
                            self.dynamic_attributes.push(attr);
                            self.attr_paths.push(self.current_path.clone());
                            TemplateAttribute::Dynamic { id: ct }
                        }
                    })
                    .collect();

                let children: Vec<_> = el
                    .children
                    .iter()
                    .enumerate()
                    .map(|(idx, root)| {
                        self.current_path.push(idx as u8);
                        let out = self.leak_node(root);
                        self.current_path.pop();
                        out
                    })
                    .collect();

                // TemplateNode::Element {
                //     tag: dioxus_elements::#el_name::TAG_NAME,
                //     namespace: dioxus_elements::#el_name::NAME_SPACE,
                //     attrs: &[ #attrs ],
                //     children: &[ #children ],
                // }
                TemplateNode::Element {
                    tag: "todo",
                    namespace: None,
                    attrs: Box::leak(static_attrs.into_boxed_slice()),
                    children: Box::leak(children.into_boxed_slice()),
                }
            }

            BodyNode::Text(text) if text.is_static() => {
                let text = text.source.as_ref().unwrap();
                TemplateNode::Text {
                    text: Box::leak(text.value().into_boxed_str()),
                }
            }

            BodyNode::RawExpr(_)
            | BodyNode::Text(_)
            | BodyNode::ForLoop(_)
            | BodyNode::IfChain(_)
            | BodyNode::Component(_) => {
                let ct = self.dynamic_nodes.len();
                self.dynamic_nodes.push(root);
                self.node_paths.push(self.current_path.clone());

                match root {
                    BodyNode::Text(_) => TemplateNode::DynamicText { id: ct },
                    _ => TemplateNode::Dynamic { id: ct },
                }
            }
        }
    }

    fn render_static_node(&mut self, root: &'a BodyNode) -> TokenStream2 {
        match root {
            BodyNode::Element(el) => {
                let el_name = &el.name;

                // dynamic attributes
                // [0]
                // [0, 1]
                // [0, 1]
                // [0, 1]
                // [0, 1, 2]
                // [0, 2]
                // [0, 2, 1]

                let static_attrs = el.attributes.iter().map(|attr| match &attr.attr {
                    ElementAttr::AttrText { name, value } if value.is_static() => {
                        let value = value.source.as_ref().unwrap();
                        quote! {
                            ::dioxus::core::TemplateAttribute::Static {
                                name: dioxus_elements::#el_name::#name.0,
                                namespace: dioxus_elements::#el_name::#name.1,
                                value: #value,

                                // todo: we don't diff these so we never apply the volatile flag
                                // volatile: dioxus_elements::#el_name::#name.2,
                            }
                        }
                    }

                    ElementAttr::CustomAttrText { name, value } if value.is_static() => {
                        let value = value.source.as_ref().unwrap();
                        quote! {
                            ::dioxus::core::TemplateAttribute::Static {
                                name: #name,
                                namespace: None,
                                value: #value,

                                // todo: we don't diff these so we never apply the volatile flag
                                // volatile: dioxus_elements::#el_name::#name.2,
                            }
                        }
                    }

                    ElementAttr::AttrExpression { .. }
                    | ElementAttr::AttrText { .. }
                    | ElementAttr::CustomAttrText { .. }
                    | ElementAttr::CustomAttrExpression { .. }
                    | ElementAttr::EventTokens { .. } => {
                        let ct = self.dynamic_attributes.len();
                        self.dynamic_attributes.push(attr);
                        self.attr_paths.push(self.current_path.clone());
                        quote! { ::dioxus::core::TemplateAttribute::Dynamic { id: #ct } }
                    }
                });

                let attrs = quote! { #(#static_attrs),*};

                let children = el.children.iter().enumerate().map(|(idx, root)| {
                    self.current_path.push(idx as u8);
                    let out = self.render_static_node(root);
                    self.current_path.pop();
                    out
                });

                let _opt = el.children.len() == 1;
                let children = quote! { #(#children),* };

                quote! {
                    ::dioxus::core::TemplateNode::Element {
                        tag: dioxus_elements::#el_name::TAG_NAME,
                        namespace: dioxus_elements::#el_name::NAME_SPACE,
                        attrs: &[ #attrs ],
                        children: &[ #children ],
                    }
                }
            }

            BodyNode::Text(text) if text.is_static() => {
                let text = text.source.as_ref().unwrap();
                quote! { ::dioxus::core::TemplateNode::Text{ text: #text } }
            }

            BodyNode::RawExpr(_)
            | BodyNode::Text(_)
            | BodyNode::ForLoop(_)
            | BodyNode::IfChain(_)
            | BodyNode::Component(_) => {
                let ct = self.dynamic_nodes.len();
                self.dynamic_nodes.push(root);
                self.node_paths.push(self.current_path.clone());

                match root {
                    BodyNode::Text(_) => {
                        quote! { ::dioxus::core::TemplateNode::DynamicText { id: #ct } }
                    }
                    _ => quote! { ::dioxus::core::TemplateNode::Dynamic { id: #ct } },
                }
            }
        }
    }
}

#[test]
fn template() {
    let input = quote! {
        div {
            width: 100,
            height: "100px",
            "width2": 100,
            "height2": "100px",
            p {
                "hello world"
            }
            (0..10).map(|i| rsx!{"{i}"})
        }
    };
    let call_body: CallBody = syn::parse2(input).unwrap();

    let template = call_body.leak_template("testing");

    dbg!(template);

    assert_eq!(
        template,
        Template {
            name: "testing",
            roots: &[TemplateNode::Element {
                tag: "div",
                namespace: None,
                attrs: &[
                    TemplateAttribute::Dynamic { id: 0 },
                    TemplateAttribute::Static {
                        name: "height",
                        namespace: None,
                        value: "100px",
                    },
                    TemplateAttribute::Dynamic { id: 1 },
                    TemplateAttribute::Static {
                        name: "height2",
                        namespace: None,
                        value: "100px",
                    },
                ],
                children: &[
                    TemplateNode::Element {
                        tag: "p",
                        namespace: None,
                        attrs: &[],
                        children: &[TemplateNode::Text {
                            text: "hello world",
                        }],
                    },
                    TemplateNode::Dynamic { id: 0 }
                ],
            }],
            node_paths: &[&[0, 1,],],
            attr_paths: &[&[0,], &[0,],],
        },
    )
}
