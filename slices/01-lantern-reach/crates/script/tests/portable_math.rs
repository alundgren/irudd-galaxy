//! Luau's `math` and `vector` go through `libm` and plain Rust (#42), so a script's
//! numbers are the same bits on every OS. Each input below is one where Windows' C
//! library rounds the last bit differently from `libm`; the pinned bits are `libm`'s.

use std::path::Path;
use std::rc::Rc;

use galaxy_script::read_pack;
use galaxy_script::vm::PackVm;
use mlua::{Function, Value};

fn vm() -> PackVm {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/gm-examples/kettle_growth");
    let pack = read_pack(&dir).expect("the example pack reads");
    PackVm::bare(Rc::new(pack)).expect("a bare VM")
}

/// `(expression over X and Y, X bits, Y bits, libm's result bits)`.
const CASES: &[(&str, u64, u64, u64)] = &[
    (
        "math.sin(X)",
        0x4027_f9a3_0a60_6fcc,
        0,
        0xbfe1_812d_29d9_0fa5,
    ),
    (
        "math.sin(X)",
        0x4027_947f_2f38_330a,
        0,
        0xbfe6_6be5_f8ef_d8d7,
    ),
    (
        "math.tan(X)",
        0x3fe3_f6de_952f_3e14,
        0,
        0x3fe7_0887_41b7_b72b,
    ),
    (
        "math.tan(X)",
        0x4000_917d_ad6b_f130,
        0,
        0xbffd_4554_1a8a_0313,
    ),
    (
        "math.exp(X)",
        0xc031_cb2f_f78a_e49f,
        0,
        0x3e54_1992_ed39_7940,
    ),
    (
        "math.exp(X)",
        0x4001_c826_2560_e258,
        0,
        0x4022_7709_3304_97fc,
    ),
    (
        "math.log(X)",
        0x402a_221a_d18b_296c,
        0,
        0x4004_8f7b_b723_fb50,
    ),
    (
        "math.log(X)",
        0x4038_3384_7d1e_5dcc,
        0,
        0x4009_7dc1_522a_61c0,
    ),
    (
        "math.pow(X, Y)",
        0x3ff7_1176_bb5b_0b12,
        0x4025_3938_46cb_de8f,
        0x4048_45c5_58c4_baca,
    ),
    (
        "math.pow(X, Y)",
        0x3fba_dd5e_ff2a_b9f8,
        0x3ffb_3192_3fa7_22d2,
        0x3f96_3265_7ae0_afb8,
    ),
];

#[test]
fn transcendental_functions_match_libm_bit_for_bit() {
    let vm = vm();
    for &(expr, x, y, want) in CASES {
        let f: Function = vm
            .lua()
            .load(format!("return function(X, Y) return {expr} end"))
            .eval()
            .unwrap();
        let got: f64 = f.call((f64::from_bits(x), f64::from_bits(y))).unwrap();
        assert_eq!(got.to_bits(), want, "{expr} at {:e}", f64::from_bits(x));
    }
}

#[test]
fn optimised_constant_calls_are_not_folded_with_the_c_library() {
    // `--!optimize 2` folds builtin calls on constants at compile time with the host's C
    // library. The portable functions aren't builtins, so the call runs at run time.
    let vm = vm();
    for &(expr, x, y, want) in CASES {
        let (x, y) = (f64::from_bits(x), f64::from_bits(y));
        let src = format!(
            "--!optimize 2\nreturn {}",
            expr.replace('X', &format!("{x:?}"))
                .replace('Y', &format!("{y:?}"))
        );
        let got: f64 = vm.lua().load(src.as_str()).eval().unwrap();
        assert_eq!(got.to_bits(), want, "{src}");
    }
}

#[test]
fn log_bases_and_helpers_follow_luau() {
    let vm = vm();
    let eval = |src: &str| -> f64 { vm.lua().load(src).eval().unwrap() };
    assert_eq!(eval("return math.log(8, 2)"), 3.0);
    assert_eq!(eval("return math.log(1000, 10)"), libm::log10(1000.0));
    assert_eq!(
        eval("return math.log(81, 3)").to_bits(),
        (libm::log(81.0) / libm::log(3.0)).to_bits()
    );
    assert_eq!(eval("return math.lerp(2, 4, 1)"), 4.0);
    assert_eq!(eval("return math.lerp(2, 4, 0.25)"), 2.5);
    assert_eq!(eval("return math.map(5, 0, 10, 100, 200)"), 150.0);
    assert_eq!(eval("return math.atan2(1, -1)"), libm::atan2(1.0, -1.0));
    assert_eq!(eval("return math.sqrt(2)"), 2f64.sqrt());
    assert_eq!(
        eval("return vector.magnitude(vector.create(3, 4, 12))"),
        13.0
    );
    assert_eq!(
        eval("return vector.dot(vector.create(1, 2, 3), vector.create(4, 5, 6))"),
        32.0
    );
    assert_eq!(
        eval("return vector.angle(vector.create(1, 0, 0), vector.create(0, 1, 0))"),
        libm::atan2(1.0, 0.0)
    );
    assert_eq!(
        eval(
            "return vector.angle(vector.create(1, 0, 0), vector.create(0, 1, 0), vector.create(0, 0, -1))"
        ),
        -libm::atan2(1.0, 0.0)
    );
    assert_eq!(
        eval("local v = vector.cross(vector.create(1, 0, 0), vector.create(0, 1, 0)) return v.z"),
        1.0
    );
    assert_eq!(
        eval("local v = vector.normalize(vector.create(0, 3, 4)) return v.y"),
        f64::from(3f32 * (1.0 / 5f32))
    );
    assert_eq!(
        eval(
            "local v = vector.lerp(vector.create(0, 0, 0), vector.create(2, 4, 8), 0.5) return v.z"
        ),
        4.0
    );
}

#[test]
fn noise_is_gone() {
    let vm = vm();
    let v: Value = vm.lua().load("return math.noise").eval().unwrap();
    assert!(v.is_nil());
}
