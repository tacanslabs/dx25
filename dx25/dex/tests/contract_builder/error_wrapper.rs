use multiversx_sc_scenario::whitebox::TxResult;

pub trait TestResult {
    fn assert_failed(&self, message: &str);
}

impl TestResult for TxResult {
    #[track_caller]
    fn assert_failed(&self, message: &str) {
        assert!(
            self.result_status == 4,
            "Tx error status mismatch. Want a user error with status 4. Have status {}",
            self.result_status,
        );

        assert!(
            self.result_message.as_str().ends_with(message),
            "Tx error message mismatch. Want message to contain \"{}\". Have message \"{}\"",
            message,
            self.result_message.as_str()
        );
    }
}
