extends Node
class_name Thruster

var heat := 0.0

# Same name as a.gd's own _physics_process — see a.gd's comment. Must also
# fire, independently of a.gd's own finding.
func _physics_process(delta: float) -> void:
	heat = heat + delta * 2.0
