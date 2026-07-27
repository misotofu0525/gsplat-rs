use std::fs;

use super::q3_m4::verify_parity;

const SCHEMA: &str = "gsplat-q3-a065-element-parity/v1";
const CELL: &str = "Q3.A065.PackedCpuExact.ScalarVsNeon.ElementParity";

fn receipt() -> (String, bool) {
    // This calls the same private forced Scalar and Neon leaves used by the
    // production Packed CPU sorter. Compiling this test for AArch64 and running
    // it on the physical endpoint is the proof that both kernels executed; an
    // image or count comparison cannot substitute for this element oracle.
    let parity = verify_parity();
    let accepted = parity.all();
    let decision = if accepted { "Accepted" } else { "Rejected" };
    let reason = if accepted {
        "physical_aarch64_scalar_neon_element_parity"
    } else {
        "scalar_neon_element_parity_failed"
    };
    (
        format!(
            "{{\"schema\":\"{SCHEMA}\",\"cell\":\"{CELL}\",\"decision\":\"{decision}\",\"reason\":\"{reason}\",\"target_arch\":\"{}\",\"neon_required_by_target\":true,\"kernels\":{{\"scalar\":{{\"executed\":true,\"entry\":\"radix_sort_desc_u64_key_bits_scalar_for_test\"}},\"neon\":{{\"executed\":true,\"entry\":\"radix_sort_desc_u64_key_bits_neon_for_test\"}}}},\"correctness\":{{{}}},\"whole_plan_promotion\":false}}",
            std::env::consts::ARCH,
            parity.json_fields(),
        ),
        accepted,
    )
}

#[test]
#[ignore = "physical A065 qualification; use collect-q3-a065-simd.py"]
fn q3_a065_forced_scalar_neon_element_parity() {
    let path = std::env::var_os("GSPLAT_Q3_A065_PARITY_RECEIPT")
        .expect("GSPLAT_Q3_A065_PARITY_RECEIPT is required for the collector");
    let (contents, accepted) = receipt();
    fs::write(path, contents).expect("write Q3 A065 element-parity receipt");
    assert!(
        accepted,
        "forced Neon sorting differs from the Scalar oracle"
    );
}
