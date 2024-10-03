use syn::{parse_macro_input, DeriveInput};

mod decode;
mod encode;

#[proc_macro_derive(PadeEncode, attributes(pade_width, pade_ignore))]
pub fn pade_encode_fn(raw: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(raw as DeriveInput);
    encode::build_encode(input)
}

#[proc_macro_derive(PadeDecode, attributes(pade_width, pade_ignore))]
pub fn pade_decode_fn(raw: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(raw as DeriveInput);
    decode::build_decode(input)
}
