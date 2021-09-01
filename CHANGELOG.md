# Changelog

In this document, all remarkable changes are listed. Not mentioned are smaller code cleanups or documentation improvements.

## Unreleased

- fixed a bug where p2p sessions would falsely skip frames even when there able to run the frame
- implemented some first steps towards WASM compatability

## 0.4.3

- changed license from MIT to MIT or Apache 2.0 at the users option
- added `local_player_handle()` to `P2PSession`, which returns the handle of the local player
- added `set_fps(desired_fps)` to `P2PSpectatorSession`

## 0.4.2

- users are now allowed to save `None` buffers for a `GGRSRequest::SaveRequest`. This allows users to keep their own state history and load/save more efficiently
- added `num_players()`, `input_size()` getters to all sessions

## 0.4.1

- added sparse saving feature `P2PSession`, minimizing the SaveState requests to a bare minimum at the cost of potentially longer rollbacks
- added `set_sparse_saving()` to `P2PSession` to enable sparse saving
- added `set_fps(desired_fps)` to `P2PSession` for the user to set expected update frequency. This is helpful for frame synchronization between sessions
- fixed a bug where a spectator would not handle disconnected players correctly with more than two players
- fixed a bug where changes to `disconnect_timeout` and `disconnect_notify_start` would change existings endpoints, but would not influence endpoints created afterwards
- expanded the BoxGame example for up to four players and as many spectators as wanted
- minor code optimizations

## 0.4.0

- spectators catch up by advancing the frame twice per `advance_frame(...)` call, if too far behind
- added `frames_behind_host()` to `P2PSpectatorSession`, allowing to query how many frames the spectator client is behind the last received input
- added `set_max_frames_behind(desired_value)`to `P2PSpectatorSession`, allowing to set after how many frames behind the spectator fast-forwards to catch up
- added `set_catchup_speed(desired_value)` to `P2PSpectatorSession`, allowing to set how many frames the spectator catches up per `advance_frame()` call, if too far behind
- in `SyncTestSession`, the user now can (and has to) provide input for all players in order to advance the frame

## 0.3.0

- `GGRSError::InvalidRequest` now has an added `info` field to explain the problem in more detail
- removed unused `GGRSError::GeneralFailure`
- removed multiple methods in `SyncTestSession`, as they didn't fulfill any meaningful purpose
- removed unused sequence number from message header, fixing related issues
- fixed an issue where out-of-order packets would cause a crash
- other minor improvements

## 0.2.5

- when a player disconnects, the other players now rollback to that frame. This is done in order to eliminate wrong predictions and resimulate the game with correct disconnection indicators
- spectators now also handle those disconnections correctly

## 0.2.4

- fixed an issue where the spectator would assign wrong frames to the input
- players disconnecting now leads to a rollback to the disconnect frame, so wrongly made predictions can be removed
- in the box game example, disconnected players now spin
- minor code and documentation cleanups

## 0.2.3

- fixed an issue where encoding/decoding reference would not match, leading to client desyncs

## 0.2.2

- SyncTestSession now actually compares checksums again
- if the user doesn't provide checksums, GGRS computes a fletcher16 checksum
- internal refactoring/renaming

## 0.2.1

- fixed an issue where the spectator would only handle one UDP packet and drop the rest

## 0.2.0

- Reworked API: Instead of the user passing a GGRSInterface trait object, GGRS now returns a list of GGRSRequests for the user to fulfill
