use super::run_powershell;

#[test]
fn test_powershell_preview_captures_output_and_exits() {
    let output = run_powershell("Write-Output '预览完成'; [Console]::Error.WriteLine('diagnostic')").unwrap();
    assert!(output.contains("stdout: 预览完成"), "{output}");
    assert!(output.contains("stderr: diagnostic"), "{output}");
}

#[test]
fn test_powershell_preview_reports_prompt_error_without_waiting_for_input() {
    let error = run_powershell("Read-Host 'Name'").unwrap_err();
    assert!(error.contains("Read-Host") || error.contains("NonInteractive"), "{error}");
}

#[test]
fn test_powershell_preview_reports_exit_code_and_script_diagnostics() {
    let error = run_powershell("[Console]::Error.WriteLine('script failure'); exit 7").unwrap_err();
    assert!(error.contains('7') && error.contains("stderr: script failure"), "{error}");
}
