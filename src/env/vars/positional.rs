//! Positional parameters (`$1`, `$2`, ...) of the current scope.

use super::VarStore;

impl VarStore {
    /// Get the current scope's positional parameters.
    pub fn positional_params(&self) -> &[String] {
        let scope = self.scopes.last().unwrap();
        &scope.positional_params[scope.positional_offset..]
    }

    /// Set the current scope's positional parameters.
    pub fn set_positional_params(&mut self, params: Vec<String>) {
        let scope = self.scopes.last_mut().unwrap();
        scope.positional_params = params;
        scope.positional_offset = 0;
    }

    /// Drop the first `n` positional parameters (POSIX `shift n`).
    /// Returns `false` (and changes nothing) when `n` exceeds `$#`.
    pub fn shift_positional_params(&mut self, n: usize) -> bool {
        let scope = self.scopes.last_mut().unwrap();
        let remaining = scope.positional_params.len() - scope.positional_offset;
        if n > remaining {
            return false;
        }
        scope.positional_offset += n;
        // Reclaim consumed strings once they dominate the buffer so a
        // long-running loop does not pin the whole original argv.
        if scope.positional_offset >= 64
            && scope.positional_offset * 2 >= scope.positional_params.len()
        {
            scope.positional_params.drain(..scope.positional_offset);
            scope.positional_offset = 0;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_positional_params_is_offset_based_and_compacts() {
        let mut vs = VarStore::new();
        let params: Vec<String> = (0..200).map(|i| i.to_string()).collect();
        vs.set_positional_params(params);
        assert!(vs.shift_positional_params(1));
        assert_eq!(vs.positional_params()[0], "1");
        assert_eq!(vs.positional_params().len(), 199);
        for _ in 0..150 {
            assert!(vs.shift_positional_params(1));
        }
        assert_eq!(vs.positional_params().len(), 49);
        assert_eq!(vs.positional_params()[0], "151");
        assert_eq!(vs.positional_params()[48], "199");
        assert!(
            !vs.shift_positional_params(50),
            "over-shift must be rejected"
        );
        assert_eq!(vs.positional_params().len(), 49);
        assert!(vs.shift_positional_params(49));
        assert!(vs.positional_params().is_empty());
        assert!(vs.shift_positional_params(0));
        vs.set_positional_params(vec!["x".into()]);
        assert_eq!(vs.positional_params(), ["x".to_string()]);
    }
}
