use itertools::multiunzip;
use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::{
    spanned::Spanned, Data, DataEnum, DataStruct, DeriveInput, Fields, Generics, Ident, Index, Type
};

pub fn build_decode(input: DeriveInput) -> proc_macro::TokenStream {
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

    let (assigned_name, default_name, field_decoders, tys): (Vec<TokenStream>, Vec<TokenStream>,Vec<TokenStream>, Vec<Type>) = multiunzip(field_list
        .iter()
        .enumerate()
        .map(|(idx, f)| {
            let (name, default_name) = f
                .ident
                .as_ref()
                .map(|i| {
                        let id  = format_ident! ("field_{}", i);
                        (quote! { #id }, quote! { #i })
                })
                .unwrap_or_else(|| {
                        let i  = format_ident! ("field_{}", idx );
                        (quote! { #i }, quote! { #idx })
                });

            let field_type = &f.ty;
            // See if we've been given an encoding width override
            let decode_command = f
                .attrs
                .iter()
                .find(|attr| attr.path().is_ident("pade_width"))
                .map(|attr| {
                    attr.parse_args::<Literal>()
                        // If we find our literal, set it to do our encode with width
                        .map(|w| {
                            quote! {
                                // value is some if we have a enum varient.
                                let is_enum = Some(<#field_type as pade::PadeEncode>::PADE_VARIANT_MAP_BITS).filter(|b| b != &0);
                                let #name = if let Some(e) = is_enum {
                                    // the split here naturally will extract out the bitmap fields
                                        let decode = bitmap.split_off(bitmap_bits - e);
                                        bitmap_bits -= e;
                                        let var_e: u8 = pade::bitvec::field::BitField::load_be(&decode);
                                     <#field_type>::pade_decode_with_width(buf, #w, Some(var_e))?
                                } else {
                                     <#field_type>::pade_decode_with_width(buf, #w, None)?
                                };
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
                    || quote! {
                        let is_enum = Some(<#field_type as pade::PadeEncode>::PADE_VARIANT_MAP_BITS).filter(|b| b != &0);
                        let #name = if let Some(e) = is_enum {
                            // the split here naturally will extract out the bitmap fields
                            let decode = bitmap.split_off(bitmap_bits - e);
                            bitmap_bits -= e;
                            let var_e: u8 = pade::bitvec::field::BitField::load_be(&decode);
                             <#field_type>::pade_decode(buf, Some(var_e))?
                        } else {
                             <#field_type>::pade_decode(buf, None)?
                        };
                    }
                );

                (name, default_name, decode_command, field_type.clone())
        }));

    let struct_building = if matches!(s.fields, Fields::Unnamed(_)) {
        quote! (
              Ok(Self(#(#assigned_name),*))
        )
    } else {
        quote! (
              Ok(Self {
                  #(
                      #default_name: #assigned_name,
                  )*
              })
        )
    };

    quote! (
      #[automatically_derived]
      impl #impl_gen pade::PadeDecode for #name #ty_gen #where_clause {
          fn pade_decode(buf: &mut &[u8], var: Option<u8>) -> Result<Self, pade::PadeDecodeError> {
              let mut bitmap_bits = 0usize;
              #(
                  bitmap_bits +=
                  <#tys as pade::PadeEncode>::PADE_VARIANT_MAP_BITS;
              )*
             let bitmap_bytes = bitmap_bits.div_ceil(8);
              let mut bitmap = pade::bitvec::vec::BitVec::<u8, pade::bitvec::order::Msb0>::from_slice(&buf[0..bitmap_bytes]);
              bitmap = bitmap.split_off(bitmap_bytes * 8 - bitmap_bits);
              *buf = &buf[bitmap_bytes..];

              #(#field_decoders)*

              #struct_building
          }

            fn pade_decode_with_width(buf: &mut &[u8], width: usize, var: Option<u8>) -> Result<Self, pade::PadeDecodeError>
            where
                Self: Sized
            {
                todo!("decode width not supported for non primitives")
            }
      }
    )
}

fn build_enum_impl(name: &Ident, generics: &Generics, e: &DataEnum) -> TokenStream {
    let (impl_gen, ty_gen, where_clause) = generics.split_for_impl();
    // Each variant gets a clause in the match
    let branches = e.variants.iter().enumerate().map(|(i, v)| {
        let raw_number = number_to_literal(i);

        let name = &v.ident;
        match v.fields {
            Fields::Named(ref fields) => {
                let unnamed_fields = fields.named.iter().map(|f| {
                    let name = f.ident.as_ref().unwrap();
                    let ty = &f.ty;

                    (
                        name,
                        quote! (
                            let #name = <#ty>::pade_decode(buf, None)?;
                        )
                    )
                });

                let (field_names, field_decoders): (Vec<&Ident>, Vec<TokenStream>) =
                    unnamed_fields.unzip();

                quote! {
                    #raw_number => {
                        #(#field_decoders)*

                        Ok(Self::#name {
                            #(#field_names),*
                        })
                    }
                }
            }
            Fields::Unnamed(ref fields) => {
                let unnamed_fields = fields.unnamed.iter().enumerate().map(|(i, f)| {
                    let num = Index::from(i);
                    let field_name = format_ident!("field_{}", num);
                    let ty = &f.ty;
                    let field_encoder = quote_spanned! {f.span()=>
                            let #field_name = <#ty>::pade_decode(buf, None)?;
                    };
                    (field_name, field_encoder)
                });
                let (field_names, field_decoders): (Vec<Ident>, Vec<TokenStream>) =
                    unnamed_fields.unzip();
                quote! {
                    #raw_number => {
                        #(#field_decoders)*

                        Ok(Self::#name(
                            #(#field_names),*
                        ))
                    }
                }
            }
            Fields::Unit => {
                quote! {
                    #raw_number => {
                        Ok(Self::#name)
                    }
                }
            }
        }
    });

    quote! {
        #[automatically_derived]
        impl #impl_gen pade::PadeDecode for #name #ty_gen #where_clause {
            fn pade_decode(buf: &mut &[u8], var: Option<u8>) -> Result<Self, pade::PadeDecodeError>
            where
                Self: Sized
            {
                // the variant will either be the first byte or passed in
                let variant = var.unwrap_or_else(|| {
                    let ch = buf[0];
                    *buf = &buf[1..];
                    ch
                });

                match variant {
                    #(#branches)*
                    _ => return Err(pade::PadeDecodeError::InvalidEnumVariant(variant))
                }

            }

            fn pade_decode_with_width(buf: &mut &[u8], width: usize, var: Option<u8>) -> Result<Self, pade::PadeDecodeError>
            where
                Self: Sized
            {
                todo!("decode width not supported for enums")
            }
        }
    }
}

fn number_to_literal(value: usize) -> Literal {
    Literal::u8_unsuffixed(value.to_le_bytes()[0])
}
