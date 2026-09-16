extends Node
class_name Player

var seeded_rng := RandomNumberGenerator.new()

# unseeded_rng fires here unconditionally — a bare global RNG read.
func roll_damage() -> int:
	return randi_range(1, 6)

# Must NOT fire: a member call on a project-owned, seedable RNG instance
# is a different shape than the bare global call above — same real
# distinction found dogfooding against a real addon's own deterministic
# RNG wrapper (see main.rs's RNG_FUNCS comment).
func roll_seeded() -> int:
	return seeded_rng.randi_range(1, 6)

# wallclock_read fires here unconditionally — no reachability scoping
# needed, same as unseeded_rng.
func log_event() -> void:
	var now := Time.get_ticks_msec()
	print("event at %d" % now)

# Must NOT fire: a plain user-defined method that happens to be named
# get_ticks_msec on an unrelated type is not Time.get_ticks_msec.
func get_ticks_msec() -> int:
	return 0

func call_local_get_ticks_msec() -> int:
	return get_ticks_msec()
