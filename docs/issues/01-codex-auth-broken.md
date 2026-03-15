# Issue: Codex authentication is broken

## Observation:

Opening the provided link gives an error on auth.openai.com:

Authentication Error
An error occurred during authentication (unknown_error). Please try again.
You can contact us through our help center at help.openai.com if you keep seeing this error. (Please include the request ID 2a60ff3e-d7b3-460d-9b00-10fb493f463c in your email.)

## Expected behavior:

Able to complete authentication

## Analysis:

The OAuth server performs an **exact string match** on the `redirect_uri`
parameter against the URIs registered for the client application
(`app_EMoamEEZ73f0CkXaXp7hrann`).

The registered URI is `http://localhost:1455/auth/callback`.
Our code was sending `http://127.0.0.1:1455/auth/callback`.

Despite `localhost` and `127.0.0.1` being functionally equivalent on most
systems, the auth server treats them as distinct strings and rejects the
request with `unknown_error`.

Reference: `packages/ai/src/utils/oauth/openai-codex.ts` in pi-mono uses
`http://localhost:1455/auth/callback` and works correctly.

## Solution:

Changed `REDIRECT_URI` in `src/auth/codex.rs` from
`http://127.0.0.1:1455/auth/callback` to `http://localhost:1455/auth/callback`.

The local callback listener continues to bind on `127.0.0.1:1455`
(which `localhost` resolves to), so no networking change is required.

## Status: Fixed
