use super::*;

#[test]
fn msa_token_parsing() {
    let tokens = parse_msa_tokens(
        br#"{"token_type":"Bearer","scope":"XboxLive.SignIn","expires_in":3600,
            "access_token":"EwA-access","refresh_token":"M.R3-refresh"}"#,
    )
    .unwrap();
    assert_eq!(tokens.access_token.expose(), "EwA-access");
    assert_eq!(tokens.refresh_token.unwrap().expose(), "M.R3-refresh");
    assert!(tokens.expires_at > Utc::now() + Duration::seconds(3500));

    let no_rotation =
        parse_msa_tokens(br#"{"token_type":"bearer","expires_in":60,"access_token":"a"}"#).unwrap();
    assert!(no_rotation.refresh_token.is_none());

    for bad in [
        &br#"{"token_type":"Bearer","expires_in":3600}"#[..],
        br#"{"token_type":"mac","expires_in":3600,"access_token":"a"}"#,
        br#"{"token_type":"Bearer","expires_in":0,"access_token":"a"}"#,
        br#"{"token_type":"Bearer","access_token":"a"}"#,
        br#"{"token_type":"Bearer","expires_in":60,"access_token":"a","refresh_token":5}"#,
        b"not json",
    ] {
        assert_eq!(
            parse_msa_tokens(bad).err().map(|e| e.code),
            Some(ErrorCode::AuthProtocolError)
        );
    }
}

#[test]
fn msa_errors_are_classified_without_descriptions() {
    let body = br#"{"error":"invalid_grant","error_description":"AADSTS70000: secret trace id",
                   "error_codes":[70000]}"#;
    let error = classify_msa_error(400, body);
    assert_eq!(error.code, ErrorCode::AuthReauthRequired);
    let text = serde_json::to_string(&error).unwrap();
    assert!(!text.contains("trace"), "{text}");
    assert!(text.contains("70000"));

    assert_eq!(
        classify_msa_error(400, br#"{"error":"unauthorized_client"}"#).code,
        ErrorCode::AuthForbidden
    );
    assert_eq!(
        classify_msa_error(503, br#"{"error":"temporarily_unavailable"}"#).code,
        ErrorCode::ServiceUnavailable
    );
    assert_eq!(
        classify_msa_error(502, b"<html>").code,
        ErrorCode::ServiceUnavailable
    );
    assert_eq!(classify_msa_error(429, b"").code, ErrorCode::RateLimited);
}

#[test]
fn request_bodies_match_the_contract() {
    let user = xbox_user_request("EwA-token");
    assert_eq!(user["Properties"]["AuthMethod"], "RPS");
    assert_eq!(user["Properties"]["SiteName"], "user.auth.xboxlive.com");
    assert_eq!(user["Properties"]["RpsTicket"], "d=EwA-token");
    assert_eq!(user["RelyingParty"], "http://auth.xboxlive.com");
    assert_eq!(user["TokenType"], "JWT");

    let xsts = xsts_request("user-token");
    assert_eq!(xsts["Properties"]["SandboxId"], "RETAIL");
    assert_eq!(xsts["Properties"]["UserTokens"], json!(["user-token"]));
    assert_eq!(xsts["RelyingParty"], "rp://api.minecraftservices.com/");

    let login = minecraft_login_request("1234", "xsts\"token");
    assert_eq!(login["xtoken"], "XBL3.0 x=1234;xsts\"token");
    assert_eq!(login["platform"], "PC_LAUNCHER");

    let form = code_exchange_form("cid", "code&x=1", "http://localhost:5000/", "verifier");
    assert!(form.contains(&("code", "code&x=1".to_owned())));
    assert!(form.contains(&("code_verifier", "verifier".to_owned())));
    assert!(form.contains(&("grant_type", "authorization_code".to_owned())));
    assert!(!form.iter().any(|(k, _)| *k == "client_secret"));
    let refresh = refresh_form("cid", "r");
    assert!(refresh.contains(&("grant_type", "refresh_token".to_owned())));
}

#[test]
fn xbox_token_parsing() {
    let body = br#"{"IssueInstant":"2026-10-16T10:00:00.0000000Z","NotAfter":"2026-10-17T02:00:00.0000000Z",
        "Token":"xsts-token","DisplayClaims":{"xui":[{"uhs":"1234567890","xid":"2535400000000000"}]}}"#;
    let token = parse_xbox_token(body, "xsts").unwrap();
    assert_eq!(token.token.expose(), "xsts-token");
    assert_eq!(token.user_hash, "1234567890");
    assert_eq!(token.xuid.as_deref(), Some("2535400000000000"));

    let no_xid =
        br#"{"NotAfter":"2026-10-17T02:00:00Z","Token":"t","DisplayClaims":{"xui":[{"uhs":"9"}]}}"#;
    assert!(
        parse_xbox_token(no_xid, "xbox_user")
            .unwrap()
            .xuid
            .is_none()
    );

    for bad in [
        &br#"{"NotAfter":"2026-10-17T02:00:00Z","Token":"t","DisplayClaims":{"xui":[]}}"#[..],
        br#"{"NotAfter":"yesterday","Token":"t","DisplayClaims":{"xui":[{"uhs":"9"}]}}"#,
        br#"{"NotAfter":"2026-10-17T02:00:00Z","DisplayClaims":{"xui":[{"uhs":"9"}]}}"#,
    ] {
        assert!(parse_xbox_token(bad, "xsts").is_err());
    }
}

#[test]
fn xsts_errors_are_specific() {
    let cases = [
        (2_148_916_233_i64, ErrorCode::ProfileSetupRequired),
        (2_148_916_235, ErrorCode::AuthAccountRestricted),
        (2_148_916_238, ErrorCode::AuthAccountRestricted),
        (2_148_916_236, ErrorCode::AuthAccountRestricted),
        (2_148_916_234, ErrorCode::AuthAccountRestricted),
        (2_148_916_999, ErrorCode::AuthForbidden),
    ];
    for (xerr, code) in cases {
        let body = format!(
            r#"{{"Identity":"0","XErr":{xerr},"Message":"","Redirect":"https://start.ui.xboxlive.com"}}"#
        );
        let error = classify_xsts_error(401, body.as_bytes());
        assert_eq!(error.code, code, "{xerr}");
    }
    assert_eq!(
        classify_xsts_error(500, b"").code,
        ErrorCode::ServiceUnavailable
    );
}

#[test]
fn minecraft_token_parsing() {
    let token = parse_minecraft_token(
        br#"{"username":"0f1e2d3c-uuid","roles":[],"metadata":{},"access_token":"eyJ.mc","expires_in":86400,"token_type":"Bearer"}"#,
    )
    .unwrap();
    assert_eq!(token.access_token.expose(), "eyJ.mc");
    assert!(parse_minecraft_token(br#"{"access_token":"a"}"#).is_err());
}

#[test]
fn only_the_recognized_registration_error_means_not_approved() {
    let not_approved = br#"{"path":"/launcher/login","errorType":"FORBIDDEN","error":"FORBIDDEN",
        "errorMessage":"Invalid app registration, see https://aka.ms/AppRegInfo for more information"}"#;
    assert_eq!(
        classify_minecraft_error(403, not_approved, "minecraft_login").code,
        ErrorCode::AuthAppNotApproved
    );
    let other = br#"{"errorType":"FORBIDDEN","errorMessage":"Something else"}"#;
    assert_eq!(
        classify_minecraft_error(403, other, "minecraft_login").code,
        ErrorCode::AuthForbidden
    );
    assert_eq!(
        classify_minecraft_error(403, b"", "entitlements").code,
        ErrorCode::AuthForbidden
    );
    assert_eq!(
        classify_minecraft_error(401, b"", "entitlements").code,
        ErrorCode::AuthReauthRequired
    );
    assert_eq!(
        classify_minecraft_error(404, br#"{"errorType":"NOT_FOUND"}"#, "profile").code,
        ErrorCode::ProfileSetupRequired
    );
    assert_eq!(
        classify_minecraft_error(429, b"", "entitlements").code,
        ErrorCode::RateLimited
    );
    assert_eq!(
        classify_minecraft_error(503, b"", "profile").code,
        ErrorCode::ServiceUnavailable
    );
}

#[test]
fn entitlements_require_a_strict_shape() {
    let owned = br#"{"items":[{"name":"product_minecraft","signature":"x"},{"name":"game_minecraft","signature":"y"}],
        "signature":"z","keyId":"1"}"#;
    assert_eq!(
        parse_entitlements(owned).unwrap(),
        Entitlements {
            play: true,
            ownership: true
        }
    );
    let subscription = br#"{"items":[{"name":"game_minecraft","source":"GAMEPASS"}]}"#;
    assert_eq!(
        parse_entitlements(subscription).unwrap(),
        Entitlements {
            play: true,
            ownership: false
        }
    );
    let none = br#"{"items":[]}"#;
    assert!(!parse_entitlements(none).unwrap().play);
    let ownership_only = br#"{"items":[{"name":"product_minecraft"}]}"#;
    assert!(!parse_entitlements(ownership_only).unwrap().play);
    for bad in [
        &b"{}"[..],
        br#"{"items":{}}"#,
        br#"{"items":[{"id":1}]}"#,
        b"[]",
    ] {
        assert_eq!(
            parse_entitlements(bad).err().map(|e| e.code),
            Some(ErrorCode::AuthProtocolError)
        );
    }
}

#[test]
fn profile_parsing() {
    let profile = parse_profile(
        br#"{"id":"0F1E2D3C4B5A69788796A5B4C3D2E1F0","name":"Player_1","skins":[],"capes":[]}"#,
    )
    .unwrap();
    assert_eq!(profile.id, "0f1e2d3c4b5a69788796a5b4c3d2e1f0");
    assert_eq!(profile.name, "Player_1");
    for bad in [
        &br#"{"id":"short","name":"P"}"#[..],
        br#"{"id":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","name":"bad name!"}"#,
        br#"{"id":"0f1e2d3c4b5a69788796a5b4c3d2e1f0"}"#,
    ] {
        assert!(parse_profile(bad).is_err());
    }
}
