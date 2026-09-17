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

# unordered_parallelism fires here unconditionally — no reachability
# scoping needed, same as the two rules above.
func spawn_workers() -> void:
	WorkerThreadPool.add_task(func(): pass)

# Must NOT fire: an unrelated object's own "add_task" method is not
# WorkerThreadPool.add_task — matched by the full Type.method spelling,
# same distinction wallclock_read's own negative case above relies on.
class TaskQueue:
	func add_task(job: Callable) -> void:
		job.call()

func queue_locally(queue: TaskQueue) -> void:
	queue.add_task(func(): pass)

var gravity := 9.8

# float_outside_fixed_step fires here — delta is explicitly typed float,
# so gravity * delta (and the whole velocity_y + ... chain around it)
# qualifies. Deduped to one warning on the outermost expression, not one
# per operator.
func _physics_process(delta: float) -> void:
	apply_gravity(delta)
	accumulate(delta)
	# Real, disclosed limitation: pure untyped-variable arithmetic is
	# invisible to this rule (no type inference) — must NOT fire even
	# though it's reachable from _physics_process.
	var a = 1
	var b = 2
	var c = a + b

func apply_gravity(delta: float) -> void:
	velocity_y = velocity_y + gravity * delta

var elapsed := 0.0

# Must NOT fire (a real, disclosed gap, not intentional coverage): a
# compound-assignment accumulation is a distinct AST shape from the plain
# BinaryExpr this rule walks. Found dogfooding against a real project
# (Orama-Interactive/Pixelorama's own Selection.gd `_marching_ants_time_
# elapsed += delta` inside a real _process) — same gap exists in the
# Rust/Unreal implementations too, see docs/rule-catalog.md.
func accumulate(delta: float) -> void:
	elapsed += delta

# Must NOT fire: same float-arithmetic shape as apply_gravity, but never
# called from _process/_physics_process — reachability scoping excludes
# it, same as the Rust/Unreal sides' own NotReached-style negative case.
func unrelated_math(delta: float) -> float:
	return 1.0 + delta * 2.0
