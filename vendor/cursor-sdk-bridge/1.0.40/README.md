# Cursor SDK Bridge protocol 1.0.40

These unmodified protocol definitions come from the official SDK 1.0.40 Darwin
ARM64 standalone archive, SHA256
`56f7f68a43ce73daa8d93c60b35b5301feb6a351e1dabfefe9873d80d56931cb`.
The archive manifest identifies bridge version 1.0.0 and protocol sdk.v1.

Source: [official release v1.0.40](https://github.com/cursor/sdk-bridge/releases/tag/v1.0.40).
Archive manifest source/bridge commit: `863444c1b502763bc7e22777084d20d352d5cdf4`.
The public docs/schema source at `1ba4fe546ea597c2f14b571b0ebe59ab1d20b70e`
is a separate revision. The runtime uses explicit JSON translation rather than
a generated protobuf client: control/catalog validation, durable CreateAgent and
Send, exact GetAgent/GetRun/ObserveRun reads and explicit CancelRun. Vendoring
these definitions does not prove terminal correlated responses, safe uncertain
Retry or active option-preserving recovery. See [installation and acceptance
limits](../../../docs/cursor-cloud.md).
