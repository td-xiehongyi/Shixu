use shixu_core::{contracts::error::AppError, storage::DataProtector};
use shixu_native::protection::DpapiProtector;
#[cfg(not(windows))]
#[test]
fn unsupported_platform_has_no_plaintext_fallback() {
    assert_eq!(
        DpapiProtector.protect(b"synthetic"),
        Err(AppError::Unsupported)
    );
    assert_eq!(
        DpapiProtector.unprotect(b"synthetic"),
        Err(AppError::Unsupported)
    );
    assert!(matches!(
        DpapiProtector::open_database(std::path::Path::new("unused")),
        Err(AppError::Unsupported)
    ));
}
#[test]
#[ignore = "requires actual Windows current-user DPAPI and ACL inspection"]
fn current_windows_identity_roundtrip_and_private_directory() {
    #[cfg(not(windows))]
    panic!("BLOCKED: actual Windows DPAPI/ACL environment unavailable");
    #[cfg(windows)]
    {
        let p = DpapiProtector;
        let sealed = p.protect(b"synthetic-local-data-not-a-secret").unwrap();
        assert_ne!(sealed, b"synthetic-local-data-not-a-secret");
        assert_eq!(
            p.unprotect(&sealed).unwrap(),
            b"synthetic-local-data-not-a-secret"
        );
        let mut damaged = sealed;
        let mid = damaged.len() / 2;
        damaged[mid] ^= 0xff;
        assert_eq!(p.unprotect(&damaged), Err(AppError::AuthFailed));
        let dir = std::env::temp_dir().join(format!("shixu-dpapi-{}", std::process::id()));
        let db = DpapiProtector::open_database(&dir).unwrap();
        drop(db);
        // Inspect actual DACL with Windows, including inheritance and each SQLite file.
        let script = r#"$root=$env:SHIXU_N1_ACL_ROOT; $sid=[System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value; foreach($p in @($root,(Join-Path $root 'messages.sqlite3'))){$acl=Get-Acl -LiteralPath $p; if(-not $acl.AreAccessRulesProtected -and $p -eq $root){exit 2}; foreach($ace in $acl.Access){if($ace.IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value -ne $sid){exit 3}}}"#;
        let status = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env("SHIXU_N1_ACL_ROOT", &dir)
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[test]
#[ignore = "requires synthetic DPAPI blob created by a different real Windows identity"]
fn wrong_windows_identity_fails_closed() {
    #[cfg(not(windows))]
    panic!("BLOCKED: second Windows identity and foreign DPAPI blob unavailable");
    #[cfg(windows)]
    {
        let path = std::env::var_os("SHIXU_N1_FOREIGN_DPAPI_BLOB").expect(
            "BLOCKED: supply synthetic foreign-identity DPAPI blob path; do not use real secrets",
        );
        let sealed = std::fs::read(path).unwrap();
        assert_eq!(DpapiProtector.unprotect(&sealed), Err(AppError::AuthFailed));
    }
}
