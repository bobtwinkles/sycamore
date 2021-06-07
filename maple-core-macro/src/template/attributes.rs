use std::fmt;

use proc_macro2::TokenStream;
use quote::{quote, quote_spanned, ToTokens};
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::token::Paren;
use syn::{parenthesized, Expr, Ident, Result, Token};

pub enum AttributeType {
    /// Syntax: `name`.
    DomAttribute { name: AttributeName },
    /// Syntax: `on:event`.
    Event { event: String },
    /// Syntax: `bind:value`.
    Bind { prop: String },
    /// Syntax: `ref`.
    Ref,
}

impl Parse for AttributeType {
    fn parse(input: ParseStream) -> Result<Self> {
        let ident: AttributeName = input.parse()?;
        let ident_str = ident.to_string();

        if ident_str == "ref" {
            Ok(Self::Ref)
        } else if input.peek(Token![:]) {
            let _colon: Token![:] = input.parse()?;
            match ident_str.as_str() {
                "on" => {
                    let event = input.call(Ident::parse_any)?;
                    Ok(Self::Event {
                        event: event.to_string(),
                    })
                }
                "bind" => {
                    let prop = input.call(Ident::parse_any)?;
                    Ok(Self::Bind {
                        prop: prop.to_string(),
                    })
                }
                _ => Err(syn::Error::new_spanned(
                    ident.tag,
                    format!("unknown directive `{}`", ident_str),
                )),
            }
        } else {
            Ok(Self::DomAttribute { name: ident })
        }
    }
}

pub struct Attribute {
    pub ty: AttributeType,
    pub equals_token: Token![=],
    pub expr: Expr,
}

impl Parse for Attribute {
    fn parse(input: ParseStream) -> Result<Self> {
        Ok(Self {
            ty: input.parse()?,
            equals_token: input.parse()?,
            expr: input.parse()?,
        })
    }
}

impl ToTokens for Attribute {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let expr = &self.expr;
        let expr_span = expr.span();

        match &self.ty {
            AttributeType::DomAttribute { name } => {
                let name = name.to_string();
                tokens.extend(quote_spanned! { expr_span=>
                    ::maple_core::reactive::create_effect({
                        let _el = ::std::clone::Clone::clone(&_el);
                        move || {
                            ::maple_core::generic_node::GenericNode::set_attribute(
                                &_el,
                                #name,
                                &::std::format!("{}", #expr),
                            );
                        }
                    });
                });
            }
            AttributeType::Event { event } => {
                // TODO: Should events be reactive?
                tokens.extend(quote_spanned! { expr_span=>
                    ::maple_core::generic_node::GenericNode::event(
                        &_el,
                        #event,
                        ::std::boxed::Box::new(#expr),
                    );
                });
            }
            AttributeType::Bind { prop } => {
                #[derive(Clone, Copy)]
                enum JsPropertyType {
                    Bool,
                    String,
                }

                let (event_name, property_ty) = match prop.as_str() {
                    "value" => ("input", JsPropertyType::String),
                    "checked" => ("change", JsPropertyType::Bool),
                    _ => {
                        tokens.extend(
                            syn::Error::new(
                                prop.span(),
                                &format!("property `{}` is not supported with bind:", prop),
                            )
                            .to_compile_error(),
                        );
                        return;
                    }
                };

                let value_ty = match property_ty {
                    JsPropertyType::Bool => quote! { ::std::primitive::bool },
                    JsPropertyType::String => quote! { ::std::string::String },
                };

                let convert_into_jsvalue_fn = match property_ty {
                    JsPropertyType::Bool => {
                        quote! { ::maple_core::rt::JsValue::from_bool(*signal.get()) }
                    }
                    JsPropertyType::String => {
                        quote! { ::maple_core::rt::JsValue::from_str(&::std::format!("{}", signal.get())) }
                    }
                };

                let event_target_prop = quote! {
                    ::maple_core::rt::Reflect::get(
                        &event.target().unwrap(),
                        &::std::convert::Into::<::maple_core::rt::JsValue>::into(#prop)
                    ).unwrap()
                };

                let convert_from_jsvalue_fn = match property_ty {
                    JsPropertyType::Bool => quote! {
                        ::maple_core::rt::JsValue::as_bool(&#event_target_prop).unwrap()
                    },
                    JsPropertyType::String => quote! {
                        ::maple_core::rt::JsValue::as_string(&#event_target_prop).unwrap()
                    },
                };

                tokens.extend(quote_spanned! { expr_span=> {
                    let signal: ::maple_core::reactive::Signal<#value_ty> = #expr;

                    ::maple_core::reactive::create_effect({
                        let signal = ::std::clone::Clone::clone(&signal);
                        let _el = ::std::clone::Clone::clone(&_el);
                        move || {
                            ::maple_core::generic_node::GenericNode::set_property(
                                &_el,
                                #prop,
                                &#convert_into_jsvalue_fn,
                            );
                        }
                    });

                    ::maple_core::generic_node::GenericNode::event(
                        &_el,
                        #event_name,
                        ::std::boxed::Box::new(move |event: ::maple_core::rt::Event| {
                            signal.set(#convert_from_jsvalue_fn);
                        }),
                    )
                }});
            }
            AttributeType::Ref => {
                tokens.extend(quote_spanned! { expr_span=>{
                    ::maple_core::noderef::NodeRef::set(
                        &#expr,
                        ::std::clone::Clone::clone(&_el),
                    );
                }});
            }
        }
    }
}

pub struct AttributeList {
    pub paren_token: Paren,
    pub attributes: Punctuated<Attribute, Token![,]>,
}

impl Parse for AttributeList {
    fn parse(input: ParseStream) -> Result<Self> {
        let content;
        let paren_token = parenthesized!(content in input);

        let attributes = content.parse_terminated(Attribute::parse)?;

        Ok(Self {
            paren_token,
            attributes,
        })
    }
}

/// Represents a html element tag (e.g. `div`, `custom-element` etc...).
pub struct AttributeName {
    tag: Ident,
    extended: Vec<(Token![-], Ident)>,
}

impl Parse for AttributeName {
    fn parse(input: ParseStream) -> Result<Self> {
        let tag = input.call(Ident::parse_any)?;
        let mut extended = Vec::new();
        while input.peek(Token![-]) {
            extended.push((input.parse()?, input.parse()?));
        }

        Ok(Self { tag, extended })
    }
}

impl fmt::Display for AttributeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let AttributeName { tag, extended } = self;

        write!(f, "{}", tag.to_string())?;
        for (_, ident) in extended {
            write!(f, "-{}", ident)?;
        }

        Ok(())
    }
}
