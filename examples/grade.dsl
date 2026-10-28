vi "Grade" {
  description "Classify a score with a case structure."

  panel {
    control score "Score" @(40, 60) { type: i32, default: 85 }
    indicator grade "Grade" @(40, 140) { type: string }
  }

  diagram {
    let g = case score {
      0..59 { tunnel out = "F" }
      60..89 { tunnel out = "Pass" }
      90..100 { tunnel out = "Distinction" }
      default { tunnel out = "Out of range" }
    }
    g -> grade
  }
}
