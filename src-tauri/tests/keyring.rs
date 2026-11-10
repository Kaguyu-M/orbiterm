//! Real OS keyring round trip (Windows Credential Manager / Linux Secret Service).
//! Ignored by default: it needs an unlocked keyring. Run with
//! `cargo test --test keyring -- --ignored` (on Linux, inside a session that has one).

use orbiterm_lib::secrets;

#[test]
#[ignore]
fn keyring_round_trip() {
    let id = format!("orbiterm-test-{}", std::process::id());
    assert_eq!(secrets::get_password(&id).unwrap(), None);
    secrets::set_password(&id, "mot de passe é€ 1").unwrap();
    assert_eq!(secrets::get_password(&id).unwrap().as_deref(), Some("mot de passe é€ 1"));
    assert_eq!(secrets::saved_password(&id).as_deref(), Some("mot de passe é€ 1"));
    secrets::set_password(&id, "autre").unwrap();
    assert_eq!(secrets::get_password(&id).unwrap().as_deref(), Some("autre"));
    secrets::delete_password(&id).unwrap();
    assert_eq!(secrets::get_password(&id).unwrap(), None);
    secrets::delete_password(&id).unwrap(); // deleting twice is fine
}
