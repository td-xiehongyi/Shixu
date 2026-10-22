# Git recovery snapshot — 2026-10-22

The local .git directory, root configuration, crates directory and documentation were missing. Git history was recovered from origin/codex/shixu-v0.1 at 8ac05b3ac48b55a7f1b04497fb1c9be989273715.

151 absent tracked files were restored from that commit. SHA-256 checks confirmed all 192 pre-existing tracked files remained unchanged. Surviving local source changes and launcher scripts are preserved in this snapshot. Local NapCat runtimes, credentials, databases, vaults and build output are excluded.

This is a preservation snapshot, not a validated build. Previously unpushed changes in deleted files cannot be recovered from origin. In particular, current desktop code references model_client::credential, model_client::http and start_auto_connect, which are absent from the restored older native core. The restored launch entry and documentation also reflect the older remote version. Recover newer core files and root configuration from a separate backup before treating this as the October 13 implementation.

Validation: existing tracked file hashes unchanged; git diff --check passed. pnpm typecheck could not run because the installed dependency tree does not provide tsc. Full build, tests and live application behavior were not verified.
