vi "Average" {
  description "Average a numeric array."

  panel {
    control values "Values" @(40, 60) { type: dbl[], default: [1.0, 2.0, 3.0] }
    indicator mean "Mean" @(40, 140) { type: dbl }
  }

  diagram {
    values -> array_mean -> mean
  }
}
