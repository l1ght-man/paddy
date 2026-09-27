//! Talks to the real internet, so it is ignored by default:
//!   cargo test -p paddy-net -- --ignored
use paddy_core::{verify_font_bytes, FONT_CATALOG};
use paddy_net::{fetch, fetch_verified, NetError, Policy};

#[test]
#[ignore = "needs internet"]
fn every_catalog_font_downloads_and_matches_its_pin() {
    let policy = Policy::default();
    for pack in FONT_CATALOG {
        for f in pack.files {
            let bytes =
                fetch_verified(f.url, f.sha256, f.size + 1, &policy).unwrap_or_else(|e| panic!("{}: {e}", f.url));
            verify_font_bytes(f, &bytes).unwrap_or_else(|e| panic!("{}: {e}", f.file));
            println!("ok {} ({} bytes)", f.file, bytes.len());
        }
    }
}

#[test]
#[ignore = "needs internet"]
fn a_wrong_pin_is_rejected_over_real_https() {
    let f = &FONT_CATALOG[0].files[0];
    let err = fetch_verified(f.url, &"0".repeat(64), f.size + 1, &Policy::default()).unwrap_err();
    assert!(matches!(err, NetError::Hash { .. }), "{err:?}");
    // and a limit smaller than the file fails cleanly
    assert!(matches!(fetch(f.url, 1000, &Policy::default()), Err(NetError::TooBig { .. })));
}

#[test]
#[ignore = "needs internet"]
fn plain_http_and_private_targets_are_refused_with_the_default_policy() {
    let p = Policy::default();
    assert_eq!(fetch("http://example.com/", 1000, &p).unwrap_err(), NetError::NotHttps);
    assert!(matches!(fetch("https://127.0.0.1/", 1000, &p), Err(NetError::PrivateAddress(_))));
    assert!(matches!(fetch("https://localhost/", 1000, &p), Err(NetError::PrivateAddress(_))));
}
