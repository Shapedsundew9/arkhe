use arkhe::verify_m1::run_m1a_verification_suite;

#[test]
fn fixed_structure_m1a_gates_pass_on_development_seeds() {
    let summary = run_m1a_verification_suite(0, 20);
    assert!(summary.all_passed, "{:#?}", summary.gates);
    assert_eq!(summary.gates.len(), 4);
    assert!(summary.gates.iter().all(|gate| gate.failures.is_empty()));
}
