use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::{
    spanned::Spanned, Data, DataEnum, DataStruct, DeriveInput, Fields, Generics, Ident, Index
};

pub fn build_encode(input: DeriveInput) -> proc_macro::TokenStream {
    let expanded = match input.data {
        Data::Struct(ref s) => build_struct_impl(&input.ident, &input.generics, s),
        Data::Enum(ref e) => build_enum_impl(&input.ident, &input.generics, e),
        _ => unimplemented!("Not yet able to derive on this type")
    };
    proc_macro::TokenStream::from(expanded)
}

fn build_struct_impl(name: &Ident, generics: &Generics, s: &DataStruct) -> TokenStream {
    let (impl_gen, ty_gen, where_clause) = generics.split_for_impl();

    let field_list = match s.fields {
        Fields::Named(ref fields) => &fields.named,
        Fields::Unnamed(ref fields) => &fields.unnamed,
        _ => unimplemented!()
    };

    let field_encoders: Vec<TokenStream> = field_list
        .iter()
        .enumerate()
        .map(|(idx, f)| {
            let (name, encoded, variant_map_bytes) = f
                .ident
                .as_ref()
                .map(|i| {
                    (
                        quote! { self.#i },
                        format_ident!("{}_encoded", i),
                        format_ident!("{}_variant_map_bytes", i)
                    )
                })
                .unwrap_or_else(|| {
                    let index = Index::from(idx);
                    (
                        quote! { self.#index },
                        format_ident!("field_{}_encoded", idx),
                        format_ident!("field_{}_variant_map_bytes", idx)
                    )
                });
            // See if we've been given an encoding width override
            let encode_command = f
                .attrs
                .iter()
                .find(|attr| attr.path().is_ident("pade_width"))
                .map(|attr| {
                    attr.parse_args::<Literal>()
                        // If we find our literal, set it to do our encode with width
                        .map(|w| {
                            quote_spanned! { attr.span() =>
                                let #encoded = #name.pade_encode_with_width(#w);
                            }
                        })
                        .unwrap_or_else(|_| {
                            syn::Error::new(
                                attr.span(),
                                "pade_width requires a single literal usize value"
                            )
                            .to_compile_error()
                        })
                })
                .unwrap_or_else(
                    || quote_spanned! { f.span() => let #encoded = #name.pade_encode(); }
                );
            quote! {
                #encode_command
                let #variant_map_bytes = #name.pade_variant_map_bits().div_ceil(8);
                output.extend(
                    if #variant_map_bytes > 0 {

                        let mut new_vec = pade::bitvec::view::BitView::view_bits::<pade::bitvec::order::Msb0>(
                                &#encoded[0..#variant_map_bytes]
                            ).split_at(8 - #name.pade_variant_map_bits()).1.to_vec();
                        new_vec.extend_from_bitslice(headers.as_slice());
                        headers = new_vec;

                    #encoded[#variant_map_bytes..].iter()
                } else { #encoded[0..].iter() });
            }
        })
        .collect();

    quote! {
        #[automatically_derived]
        impl #impl_gen pade::PadeEncode for #name #ty_gen #where_clause {
            fn pade_encode(&self) -> Vec<u8> {
                let mut headers = pade::bitvec::vec::BitVec::<u8, pade::bitvec::order::Msb0>::new();
                let mut output: Vec<u8> = Vec::new();
                #(#field_encoders)*

                let len = headers.len();
                if len != 0  {
                    let extra = len % 8;
                    let mut base = headers.split_off(extra);
                    base.force_align();
                    let base = base.into_vec();

                    headers.force_align();
                    let mut raw = headers.into_vec();
                    if let Some(b) = raw.first_mut() {
                        *b >>= 8 - extra
                    }
                    raw.extend(base);

                    [raw, output].concat()
                } else {
                    output
                }
            }
        }
    }
}

fn build_enum_impl(name: &Ident, generics: &Generics, e: &DataEnum) -> TokenStream {
    let (impl_gen, ty_gen, where_clause) = generics.split_for_impl();
    let variant_count = e.variants.len();
    // This will panic if there are no variants, is that a legal state?
    let variant_bits = (variant_count.ilog2() + 1) as usize;
    let variant_bytes = variant_bits.div_ceil(8);
    // Each variant gets a clause in the match
    let clauses = e.variants.iter().enumerate().map(|(i, v)| {
        let raw_number = number_to_literals(i, variant_bytes);
        let number_encoder = std::iter::once(quote! {
            [#(#raw_number),*].to_vec()
        });
        let name = &v.ident;
        match v.fields {
            Fields::Named(ref fields) => {
                let unnamed_fields = fields.named.iter().map(|f| {
                    let name = f.ident.as_ref().unwrap();
                    let field_encoder = quote_spanned! {f.span()=>
                        pade::PadeEncode::pade_encode(#name)
                    };
                    (name, field_encoder)
                });
                let (field_names, field_encoders): (Vec<&Ident>, Vec<TokenStream>) =
                    unnamed_fields.unzip();
                let all_encoders = number_encoder.chain(field_encoders);
                quote! {
                    Self::#name { #(#field_names),* } => [#(#all_encoders),*].concat()
                }
            }
            Fields::Unnamed(ref fields) => {
                let unnamed_fields = fields.unnamed.iter().enumerate().map(|(i, f)| {
                    let num = Index::from(i);
                    let field_name = format_ident!("field_{}", num);
                    let field_encoder = quote_spanned! {f.span()=>
                        pade::PadeEncode::pade_encode(#field_name)
                    };
                    (field_name, field_encoder)
                });
                let (field_names, field_encoders): (Vec<Ident>, Vec<TokenStream>) =
                    unnamed_fields.unzip();
                let all_encoders = number_encoder.chain(field_encoders);
                quote! {
                    Self::#name(#(#field_names),*) => [#(#all_encoders),*].concat()
                }
            }
            Fields::Unit => {
                quote! {
                    Self::#name => [#(#raw_number),*].to_vec()
                }
            }
        }
    });
    quote! {
        #[automatically_derived]
        impl #impl_gen pade::PadeEncode for #name #ty_gen #where_clause {
            const PADE_VARIANT_MAP_BITS: usize = #variant_bits;
            fn pade_encode(&self) -> Vec<u8> {
                match self {
                    #(#clauses),*
                }
            }
        }
    }
}

fn number_to_literals(value: usize, bytes: usize) -> Vec<Literal> {
    value.to_le_bytes()[0..bytes]
        .iter()
        .map(|n| Literal::u8_suffixed(*n))
        .collect()
}
