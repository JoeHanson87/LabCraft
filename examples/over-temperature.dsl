vi "Over Temperature" {
  description "Raises an alarm when the reading exceeds the limit."

  panel {
    control temp "Temperature" @(40, 60) { type: dbl, default: 25.0, unit: "C" }
    control limit "Limit" @(40, 130) { type: dbl, default: 30.0, unit: "C" }
    indicator alarm "Alarm" @(40, 210) { type: bool }
  }

  diagram {
    temp -> greater(limit) -> alarm
  }
}
