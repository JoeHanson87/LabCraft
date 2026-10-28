vi "Count" {
  description "Count loop iterations and publish the last value."

  panel {
    control n "Count" @(40, 60) { type: i32, default: 5 }
    indicator last "Last" @(40, 140) { type: i32 }
  }

  diagram {
    let result = for i in 0..n {
      tunnel value = add(i, 1)
    }
    result -> last
  }
}
