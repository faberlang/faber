//! C0 proof: the skeleton emits a native object exporting a constant
//! function (goal `radix-mir-clif`, unit C0).

use std::process::Command;

/// Mach-O 64-bit object magic, little-endian (`0xfeedfacf`).
const MACHO_MAGIC: [u8; 4] = [0xcf, 0xfa, 0xed, 0xfe];
/// ELF object magic.
const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

#[test]
fn emits_object_with_exported_constant_function() {
    let bytes = radix_mir_clif::emit_constant_object("faber_clif_incipit", 42)
        .expect("object emission succeeds");

    assert!(
        bytes.len() > 256,
        "object is non-trivial: {} bytes",
        bytes.len()
    );
    let is_macho = bytes.starts_with(&MACHO_MAGIC);
    let is_elf = bytes.starts_with(&ELF_MAGIC);
    assert!(is_macho || is_elf, "not a Mach-O or ELF object");

    // When `nm` is available, the exported symbol must be present.
    let dir = std::env::temp_dir().join("radix-mir-clif-c0");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("faber_clif_incipit.o");
    std::fs::write(&path, &bytes).expect("write object");
    if let Ok(output) = Command::new("nm").arg(&path).output()
        && output.status.success()
    {
        let symbols = String::from_utf8_lossy(&output.stdout);
        assert!(
            symbols.contains("faber_clif_incipit"),
            "exported symbol missing from object: {symbols}"
        );
    }
}
