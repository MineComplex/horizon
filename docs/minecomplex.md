# MineComplex fork

Changes this fork carries on top of upstream Horizon, ported from the MineComplex Java client
(`../Client`). Code: `crates/horizon-client/src/minecomplex.rs`.

## Login with the launcher's token

The MineComplex launcher starts the game with vanilla's identity arguments and its access token:

```sh
MINECOMPLEX_GAME_TOKEN=<jwt> horizon-client --username Steve --uuid <uuid> \
    --quickPlayMultiplayer play.minecomplex.ru:25565
```

`--accessToken <jwt>` works too (the environment variable wins). `--username` is the same as
`--name`, `--quickPlayMultiplayer` the same as `--server`. A JWT-shaped token (up to 4096
characters) with a valid name and UUID becomes an `AuthSession` of issuer `MineComplex`;
a Microsoft session file (`--session-file`) takes precedence when both are given.

With that session the game makes no session-service call. When the server asks for
authentication, the client only enables encryption; the proxy then sends a
`minecomplex:auth` login query and the client answers with the token's UTF-8 bytes. The token
is sent only on that channel and only after encryption is on; every other query, or a query
without a launcher token, gets an answer without a body. The proxy checks the token and
explains a missing or expired one. Without any account an online-mode server still shows
upstream's "sign in" message.

## Website skins from the CDN

Skins uploaded on the website are stored in the CDN and sent by the proxy in the unsigned
`textures` property. `skins::unpack` first asks `minecomplex::unpack_cdn`: if every texture
of the property is `https://cdn.minecomplex.net/objects/<2 hex>/<sha256>`, it is trusted as
signed, so the skin shows on the local player, on other players and on heads. Anything else
goes through the usual check. The CDN host is a constant. Horizon has no slim player model
yet, so `metadata.model` is ignored.

## Plugins

Server plugins in the spirit of Bukkit/Paper (Leaf 26.3 as the reference for the API's shape): see
[plugins.md](plugins.md). `.hmod` files in the server's `plugins/` folder, each with its data in
`plugins/<id>/`.

## Not ported

The Java client's removals (singleplayer, Realms, telemetry, friends, the multiplayer list's
Add/Edit/Delete/Direct Connection buttons, confirmation and credits screens, the high-contrast
pack) are not applied here. Upstream's fixes for the Java client's decompiler regressions
(item use, inventory transfer) do not apply to Horizon.
