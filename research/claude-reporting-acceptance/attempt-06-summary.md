# Interruption and completion-source investigation

Claude2.1.267 resumed the owned fixture using authorized subscription authentication. After actual number output began, Escape interrupted generation before1000. The native UI displayed Interrupted; /exit then exited0.

Transcript metadata contains an assistant isAbortedMidStream record and a user interruptedMessageId record sharing the prompt identity. No turn_duration was present for that interrupted prompt at the observed boundary. Successful ordinary turns and the continued-Stop example contain turn_duration records after stop_hook_summary, correlated by parentUuid ancestry to promptId. This is a candidate versioned source, not a guarantee covering tool cancellation, retry/API errors, forks or incomplete writes.

Only metadata/identity lineage is retained, with pseudonymized IDs and no message content. Missing stop/duration is not itself treated as cancellation evidence; explicit interruption metadata is required. Previous captures remain unchanged. Temporary copied credentials removed after exit.
