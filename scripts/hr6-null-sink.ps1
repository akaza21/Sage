function Get-NullSinkTargets {
  return [ordered]@{ stdout = "NUL"; stderr = "\\.\NUL" }
}
