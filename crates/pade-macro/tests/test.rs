use pade::{PadeDecode, PadeEncode};
use pade_macro::{PadeDecode, PadeEncode};

#[test]
fn can_derive_on_struct() {
    #[derive(PadeEncode, PadeDecode, PartialEq, Eq, Debug)]
    struct Test {
        index:  u128,
        vector: Vec<u128>
    }

    let test_struct = Test { index: 12345, vector: vec![123, 234, 345] };
    let bytes = test_struct.pade_encode();
    let mut slice = bytes.as_slice();
    let decoded = Test::pade_decode(&mut slice, None).unwrap();
    assert_eq!(test_struct, decoded)
}

#[test]
fn can_derive_on_unnamed_struct() {
    #[derive(PadeEncode, PadeDecode, PartialEq, Eq, Debug)]
    struct UnNamed(u128, u128);

    let test_struct = UnNamed(12345, 23456);
    let bytes = test_struct.pade_encode();
    let mut slice = bytes.as_slice();
    let decoded = UnNamed::pade_decode(&mut slice, None).unwrap();
    assert_eq!(test_struct, decoded)
}

#[test]
fn can_derive_on_nested_struct() {
    #[derive(PadeEncode, PadeDecode, PartialEq, Eq, Debug)]
    struct Inside {
        number:  u128,
        another: u128
    }

    #[derive(PadeEncode, PadeDecode, PartialEq, Eq, Debug)]
    struct Outside {
        inner: Inside
    }

    let test_struct = Outside { inner: Inside { number: 12345, another: 23456 } };
    let bytes = test_struct.pade_encode();
    let mut slice = bytes.as_slice();
    let decoded = Outside::pade_decode(&mut slice, None).unwrap();
    assert_eq!(test_struct, decoded)
}

#[test]
fn can_derive_on_basic_enum() {
    #[derive(PadeEncode, PadeDecode, PartialEq, Eq, Debug)]
    enum BasicEnum {
        StateA,
        StateB,
        StateC
    }

    let test_enum = BasicEnum::StateB;
    let bytes = test_enum.pade_encode();
    let mut slice = bytes.as_slice();
    let decoded = BasicEnum::pade_decode(&mut slice, None).unwrap();
    assert_eq!(test_enum, decoded)
}

#[test]
fn can_derive_on_enums_with_data() {
    #[derive(PadeEncode, PadeDecode, PartialEq, Eq, Debug)]
    enum DataEnum {
        Point(u128),
        Line(u128, u128, u128)
    }

    let test_enum = DataEnum::Line(12345, 23456, 34567);
    let bytes = test_enum.pade_encode();
    let mut slice = bytes.as_slice();
    let decoded = DataEnum::pade_decode(&mut slice, None).unwrap();
    assert_eq!(test_enum, decoded)
}

#[test]
fn can_derive_on_structlike_enums() {
    #[derive(PadeEncode, PadeDecode, PartialEq, Eq, Debug)]
    enum StructEnum {
        Once { x: u128, y: u128 },
        Twice { a: u128, b: u128 }
    }

    let test_enum = StructEnum::Twice { a: 12345, b: 23456 };
    let bytes = test_enum.pade_encode();
    let mut slice = bytes.as_slice();
    let decoded = StructEnum::pade_decode(&mut slice, None).unwrap();
    assert_eq!(test_enum, decoded)
}

#[test]
fn will_respect_item_width() {
    #[derive(PadeEncode, PadeDecode, PartialEq, Eq, Debug)]
    struct TooBig {
        #[pade_width(3)]
        x: i32
    }

    let test_struct = TooBig { x: 123 };
    let output = test_struct.pade_encode();
    assert_eq!(output.len(), 3, "Didn't respect byte limit");

    let mut slice = output.as_slice();
    let decoded = TooBig::pade_decode(&mut slice, None).unwrap();
    assert_eq!(test_struct, decoded)
}

#[test]
fn can_derive_on_generics() {
    #[derive(PadeEncode, PadeDecode)]
    struct GenTest<A: PadeEncode + PadeDecode> {
        numbers: Vec<u8>,
        items:   Vec<A>
    }

    #[derive(PadeEncode, PadeDecode)]
    enum GenFlatEnum<A: PadeEncode + PadeDecode> {
        Numbers(Vec<u8>),
        Items(Vec<A>)
    }

    #[derive(PadeEncode, PadeDecode)]
    enum GenStructEnum<A: PadeEncode + PadeDecode> {
        Numbers { vector: Vec<u8> },
        Items { vector: Vec<A> }
    }
}

#[test]
fn handles_odd_bool_counts() {
    // Seven bools for seven brothers
    #[derive(Default, PadeEncode)]
    struct SevenBools {
        one:   bool,
        two:   bool,
        three: bool,
        four:  bool,
        five:  bool,
        six:   bool,
        seven: bool
    }
    let seven_test = SevenBools::default();
    seven_test.pade_encode();

    #[derive(Default, PadeEncode)]
    struct EightBools {
        one:   bool,
        two:   bool,
        three: bool,
        four:  bool,
        five:  bool,
        six:   bool,
        seven: bool,
        eight: bool
    }
    let eight_test = EightBools::default();
    eight_test.pade_encode();
}
