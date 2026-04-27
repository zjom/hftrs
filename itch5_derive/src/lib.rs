use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    Data, DeriveInput, Expr, ExprLit, Fields, Ident, Lit, LitInt, MetaNameValue, Token, Type,
};

// Parses the `tag = b'S'` inside `#[itch_message(...)]`
struct ItchArgs {
    tag: u8,
}

impl Parse for ItchArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let metas: Punctuated<MetaNameValue, Token![,]> = Punctuated::parse_terminated(input)?;
        let mut tag = None;
        for m in metas {
            if m.path.is_ident("tag") {
                if let Expr::Lit(ExprLit {
                    lit: Lit::Byte(b), ..
                }) = m.value
                {
                    tag = Some(b.value());
                }
            }
        }
        tag.map(|t| ItchArgs { tag: t })
            .ok_or_else(|| syn::Error::new(input.span(), "expected `tag = b'X'`"))
    }
}

#[proc_macro_attribute]
pub fn itch_message(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as ItchArgs);
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let vis = &input.vis;

    let Data::Struct(data) = &input.data else {
        return err(&input.ident, "itch_message requires a struct");
    };
    let Fields::Named(fields_named) = &data.fields else {
        return err(&input.ident, "itch_message requires named fields");
    };

    let tag = args.tag;
    let mut accessors = Vec::<TokenStream2>::new();
    let mut total_len: usize = 1; // include the tag byte

    for field in &fields_named.named {
        let fname = field.ident.as_ref().unwrap();
        let fty = &field.ty;
        let (offset, len) = match parse_field_attr(&field.attrs) {
            Ok(v) => v,
            Err(e) => return e.to_compile_error().into(),
        };
        total_len = total_len.max(offset + len);
        accessors.push(generate_accessor(fname, fty, offset, len));
    }

    quote! {
        #vis struct #name<'a>(&'a [u8]);

        impl<'a> #name<'a> {
            pub const LEN: usize = #total_len;
            pub const TAG: u8  = #tag;

            pub fn parse(buf: &'a [u8]) -> ::core::option::Option<Self> {
                if buf.len() < Self::LEN || buf[0] != Self::TAG {
                    return ::core::option::Option::None;
                }
                ::core::option::Option::Some(Self(buf))
            }

            #(#accessors)*
        }
    }
    .into()
}

fn parse_field_attr(attrs: &[syn::Attribute]) -> syn::Result<(usize, usize)> {
    for attr in attrs {
        if !attr.path().is_ident("field") {
            continue;
        }
        let mut offset = None;
        let mut len = None;
        attr.parse_nested_meta(|meta| {
            let v: usize = meta.value()?.parse::<LitInt>()?.base10_parse()?;
            if meta.path.is_ident("offset") {
                offset = Some(v);
            } else if meta.path.is_ident("len") {
                len = Some(v);
            }
            Ok(())
        })?;
        return Ok((
            offset.ok_or_else(|| syn::Error::new_spanned(attr, "missing `offset`"))?,
            len.ok_or_else(|| syn::Error::new_spanned(attr, "missing `len`"))?,
        ));
    }
    Err(syn::Error::new(
        proc_macro2::Span::call_site(),
        "every field needs #[field(offset = N, len = M)]",
    ))
}

fn generate_accessor(name: &Ident, ty: &Type, offset: usize, len: usize) -> TokenStream2 {
    let end = offset + len;
    // Crude type dispatch — fine for a fixed protocol vocabulary.
    match quote!(#ty).to_string().as_str() {
        "u8" => quote! {
            pub fn #name(&self) -> u8 { self.0[#offset] }
        },
        "u16" => quote! {
            pub fn #name(&self) -> u16 {
                u16::from_be_bytes(self.0[#offset..#end].try_into().unwrap())
            }
        },
        "u32" => quote! {
            pub fn #name(&self) -> u32 {
                let mut b = [0u8; 4];
                b[4 - #len..].copy_from_slice(&self.0[#offset..#end]);
                u32::from_be_bytes(b)
            }
        },
        "u64" => quote! {
            pub fn #name(&self) -> u64 {
                let mut b = [0u8; 8];
                b[8 - #len..].copy_from_slice(&self.0[#offset..#end]);
                u64::from_be_bytes(b)
            }
        },
        "&[u8]" | "&'a[u8]" => quote! {
            pub fn #name(&self) -> &'a [u8] { &self.0[#offset..#end] }
        },
        _ if len == 1 => quote! {
            pub fn #name(&self) -> #ty {
                <#ty as ::core::convert::From<u8>>::from(self.0[#offset])
            }
        },
        _ => quote! {
            pub fn #name(&self) -> #ty {
                <#ty as ::core::convert::From<&[u8]>>::from(&self.0[#offset..#end])
            }
        },
    }
}

fn err(t: &impl quote::ToTokens, msg: &str) -> TokenStream {
    syn::Error::new_spanned(t, msg).to_compile_error().into()
}
