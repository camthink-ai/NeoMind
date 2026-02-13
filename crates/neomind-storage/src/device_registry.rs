let write_txn = self.db.begin_write()?;
        {
            // Update device config
            if let Some(old_type) = old_device_type {
                // ... update type index logic ...
                write_txn.commit()?;
            }
            Ok(())
        }
