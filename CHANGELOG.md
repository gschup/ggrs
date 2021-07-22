# Changelog

In this document, all remarkable changes are listed. Not mentioned are smaller code cleanups or documentation improvements.

## Unreleased

- nothing here yet...

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
