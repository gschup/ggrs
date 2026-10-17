# Late join: players joining a running session

A `P2PSession` has a fixed number of player slots. A slot can be left open when the session
starts and played later by someone who joins while the session runs. No peer rebuilds its session
or reloads its state: the joiner alone loads a state, and every other peer goes on playing.

Late join is off by default. Every peer turns it on with `SessionBuilder::with_late_join(window)`,
the joiner included. A session without it keeps no input history for joiners and sends the same
messages as before, so it can play with peers on older versions of GGRS.

## How it works

1. **Open slots.** Build the session with more slots than players, leaving the rest open with
   `SessionBuilder::add_open_slot`. An open slot's input is that of a disconnected player
   (`InputStatus::Disconnected`, default input), so the game sees nobody there.
2. **The joiner starts from a confirmed state.** Some peer (call it the sponsor) gives the joiner
   the game state of a frame `S` it has confirmed, by whatever means the game uses: GGRS does
   not serialize game state. The joiner builds its session with `with_start_frame(S)`, its own
   player added with `add_joining_local_player`, the players already playing as `Remote`, and
   the remaining slots open.
3. **Every peer adds the joiner** with `P2PSession::add_remote_player(slot, addr, S)`. Once the
   joiner's endpoint has synchronized, each peer sends it its local inputs from frame `S` on
   (each peer keeps the last `window` frames of its local inputs for this, as set by
   `with_late_join`), a window of 64 unacknowledged frames at a time. The joiner catches up by simulating those
   frames as fast as it can (`frames_behind` says how far behind it is). Nobody waits for it:
   its slot is still open.
4. **The sponsor activates the slot** with `activate_player(slot)`, once the joiner has caught up
   (the joiner can tell from `frames_behind`, and tell the sponsor by whatever means the game
   uses). This returns the activation frame `F`: the first frame the sponsor has not yet sent
   input for. Until the sponsor's connection to the joiner has synchronized, it returns
   `GgrsError::NotSynchronized`. `activate_player_at` takes a later frame: half the round trip
   to the joiner later, the joiner learns of the frame before it gets there.
   From `F` on, the slot is played; before it, it is open.
5. **Every peer learns `F` in band.** The activation frame travels in the connection status
   every input message carries, and peers pass it on. A peer cannot confirm frame `F` without the
   sponsor's input for `F`, and that input arrives with the activation, so no peer can have
   confirmed `F` without knowing who plays it. A peer that had already simulated past `F` rolls
   back to it, as it would for a misprediction. The joiner learns `F` the same way and starts
   sending input; frames between `F` and its first input hold default input, sent explicitly.

Every peer reports `GgrsEvent::PlayerActivated { player, frame }` when it learns of the
activation.

A slot whose player disconnected can be played again by someone else. The sponsor's
`activate_player` returns `GgrsError::NotSynchronized` until every peer has moved past the old
player's last frame (so none can still simulate a frame the old player played in); try again a
few frames later.

## Guarantees and limits

* Peers already playing never stop: they see the joiner like any remote player whose input they
  predict, from frame `F` on.
* Only one peer may activate a given slot, and each peer must add the joiner (with
  `add_remote_player`) before the activation reaches it; otherwise it waits for the joiner's
  input, as it would for any player whose input is missing. Choosing the sponsor and the slot is
  the game's job.
* The joiner must be added by every peer before its local inputs from `S` on fall out of their
  late join window.
* One player per late peer address. Several joiners can join at once: each joiner adds the
  others as they are announced.
* A spectator that is still replaying frames from before a slot was played again may see the
  slot's earlier player as disconnected.
* A session with late join sends its input in a new message that also has each slot's
  activation frame. Its peers must all have late join on, and its spectators must run a version
  of GGRS that knows the message (they need no setting).
