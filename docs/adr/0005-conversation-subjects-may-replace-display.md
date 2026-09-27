# Conversation subjects may replace an agent session's display name

Users need to find an Agent session by what the conversation is about. The branch name and the creation name do not do that. Following application window titles was rejected because those titles change too often, including status text such as "Thinking…".

An Agent launch may display a conversation subject for the recorded conversation. OVRCR writes that subject with a one-shot Pi RPC call, using a bounded tail of the provider history file and a Pi model named in dashboard settings. The server process reads that setting itself. The excerpt is not stored, and a crash must not leave it on disk. The row follows the recorded reference, including a Pi or Oh My Pi switch, and not a Codex history change OVRCR did not record. The subject is stored per conversation and shown until the user renames the session. Clear restores the original session name and dismisses that conversation's subject. Terminals, Grok, and Hermes are unchanged.

This supersedes the display rule in ADR 0001. It does not supersede the identity rule, and it does not restore application titles. ADR 0002 still holds: OVRCR does not persist agent transcripts.
