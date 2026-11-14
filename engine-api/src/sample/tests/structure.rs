//! The session module must not reach real scope, settings, storage or MCP state.

#[test]
fn session_sources_do_not_use_other_feature_state() {
    let sources = [
        include_str!("../session.rs"),
        include_str!("../commands.rs"),
        include_str!("../events.rs"),
        include_str!("../jobs.rs"),
    ];
    let forbidden = [
        "user_env",
        "app_db",
        "recent_scope",
        "ScopeDraft",
        "ScopeRootPort",
        "scope::",
        "SharedTableLimit",
        "settings",
        "credential",
        "mcp",
        "Channel::Cli",
        "Channel::Mcp",
        "scan::",
        "targets::",
    ];
    for source in sources {
        for word in forbidden {
            assert!(!source.contains(word), "unexpected dependency on {word}");
        }
    }
}
