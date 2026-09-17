extends Node
class_name FogEmitter

var density := 0.0

# Real bug found dogfooding against a real 250-file project
# (Orama-Interactive/Pixelorama): a whole-project function table that keyed
# by bare name alone used to let this collide with b.gd's own
# _physics_process (same name, different file) and silently drop one of
# the two bodies from scanning. Must fire — a distinct file, a distinct
# finding, not swallowed by the other file's own root of the same name.
func _physics_process(delta: float) -> void:
	density = density + delta * 0.5
