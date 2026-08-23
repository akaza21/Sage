use super::*;

impl HelperTransactionPlatform for WindowsPlatform {
    type Proof = ManagedStagingPrerequisite;

    fn read_journal(
        &mut self,
        path: &Path,
        byte_limit: usize,
    ) -> Result<Option<Vec<u8>>, ElevatedHelperError> {
        read_bounded_file(path, byte_limit)
    }

    fn create_journal(&mut self, path: &Path, bytes: &[u8]) -> Result<(), ElevatedHelperError> {
        create_initial_journal(
            path,
            bytes,
            &self.package_parent_handle,
            &self.creation_nonce,
        )
    }

    fn replace_journal(&mut self, path: &Path, bytes: &[u8]) -> Result<(), ElevatedHelperError> {
        replace_journal(path, bytes, &self.package_parent_handle)
    }

    fn remove_journal(&mut self, path: &Path) -> Result<(), ElevatedHelperError> {
        remove_exact_file(path)
    }

    fn observe(&mut self) -> Result<NamespaceObservation, ElevatedHelperError> {
        Ok(NamespaceObservation {
            stage_package: exact_directory_presence(&self.stage_package)?,
            final_package: exact_directory_presence(&self.package_target)?,
            final_state: exact_directory_presence(&self.state_target)?,
        })
    }

    fn bind_state_identity(
        &mut self,
        expected: Option<NodeIdentity>,
    ) -> Result<NodeIdentity, ElevatedHelperError> {
        let path = if exact_directory_presence(&self.state_target)? {
            &self.state_target
        } else {
            &self.stage_state
        };
        let (handle, identity) = open_mutation_directory(path, expected)?;
        self.state_handle = Some(handle);
        self.state_identity = Some(identity);
        Ok(identity)
    }

    fn revalidate_stage(&mut self) -> Result<(), ElevatedHelperError> {
        self.require_stage_root_at_path()?;
        let package_present = exact_directory_presence(&self.stage_package)?;
        let state_published = exact_directory_presence(&self.state_target)?;
        let held_package_present =
            exact_directory_presence(&self.stage_root.join(HELD_PACKAGE_NAME))?;
        let held_state_present = exact_directory_presence(&self.stage_root.join(HELD_STATE_NAME))?;
        if held_package_present != held_state_present {
            return Err(ElevatedHelperError::Drift);
        }
        validate_stage_closure(
            &self.stage_root,
            &self.stage_state,
            &self.receipt,
            package_present,
            state_published,
            held_package_present,
        )?;
        self.stage_proof = Some(self.inspect_at(self.stage_package.clone())?);
        Ok(())
    }

    fn revalidate_stage_acl(&mut self) -> Result<(), ElevatedHelperError> {
        validate_provisioned_package_acl(
            self.stage_proof
                .as_ref()
                .ok_or(ElevatedHelperError::Drift)?,
        )
        .map_err(|_| ElevatedHelperError::AclApplication)
    }

    fn revalidate_state(&mut self) -> Result<(), ElevatedHelperError> {
        self.require_bound_state_at(&self.state_target)?;
        validate_provisioned_state_root(&self.state_target)
            .map_err(|_| ElevatedHelperError::AclApplication)
    }

    fn apply_package_acl(&mut self) -> Result<(), ElevatedHelperError> {
        let proof = self
            .stage_proof
            .as_ref()
            .ok_or(ElevatedHelperError::Drift)?;
        for node in proof.held_nodes() {
            apply_acl_to_exact_node(
                node,
                &self.package_acl,
                &self.principals.administrators,
                &self.principals,
            )?;
        }
        validate_provisioned_package_acl(proof).map_err(|_| ElevatedHelperError::AclApplication)
    }

    fn publish_state_with_acl(&mut self) -> Result<(), ElevatedHelperError> {
        self.require_bound_state_at(&self.stage_state)?;
        let node = self
            .state_handle
            .as_ref()
            .ok_or(ElevatedHelperError::Drift)?;
        apply_acl(
            node,
            &self.state_acl,
            &self.principals.user,
            &self.principals,
            true,
        )?;
        self.require_bound_state_at(&self.stage_state)?;
        validate_provisioned_state_root(&self.stage_state)
            .map_err(|_| ElevatedHelperError::AclApplication)?;
        rename_exact(
            node,
            &self.state_parent_handle,
            self.state_target
                .file_name()
                .ok_or(ElevatedHelperError::HandoffIdentity)?,
            false,
        )?;
        self.require_bound_state_at(&self.state_target)?;
        validate_provisioned_state_root(&self.state_target)
            .map_err(|_| ElevatedHelperError::AclApplication)
    }

    fn publish_package_exact(&mut self) -> Result<(), ElevatedHelperError> {
        self.revalidate_stage_acl()?;
        let proof = self
            .stage_proof
            .as_ref()
            .ok_or(ElevatedHelperError::Drift)?;
        let root = proof.root_node().ok_or(ElevatedHelperError::Drift)?;
        let source = open_rename_directory(&root.path, node_identity(&root.handle)?)?;
        rename_exact(
            &source,
            &self.package_parent_handle,
            self.package_target
                .file_name()
                .ok_or(ElevatedHelperError::HandoffIdentity)?,
            false,
        )?;
        if node_identity(&source)? != node_identity(&root.handle)? {
            return Err(ElevatedHelperError::Drift);
        }
        self.stage_proof.take();
        Ok(())
    }

    fn final_managed_proof(&mut self) -> Result<Self::Proof, ElevatedHelperError> {
        inspect_provisioned_managed_staging(&self.packet)
            .map_err(|_| ElevatedHelperError::FinalProof)
    }

    fn remove_completed_stage(&mut self) -> Result<(), ElevatedHelperError> {
        let held_package = self.stage_root.join(HELD_PACKAGE_NAME);
        let held_state = self.stage_root.join(HELD_STATE_NAME);
        for held in [&held_package, &held_state] {
            if exact_directory_presence(held)? {
                remove_exact_directory_tree(held)?;
            }
        }
        self.held_package.take();
        self.held_state.take();
        self.held_state_identity.take();
        cleanup_completed_stage(&self.stage_root, &mut self.stage_root_handle)
    }

    fn rollback_package(&mut self) -> Result<(), ElevatedHelperError> {
        let root = ConfiguredManagedPackageRoot::new(self.package_target.clone())
            .map_err(|_| ElevatedHelperError::Rollback)?;
        let proof = inspect_managed_package(&self.packet, &root)
            .map_err(|_| ElevatedHelperError::Rollback)?;
        validate_provisioned_package_acl(&proof).map_err(|_| ElevatedHelperError::Rollback)?;
        let package = proof.root_node().ok_or(ElevatedHelperError::Rollback)?;
        let source = open_rename_directory(&package.path, node_identity(&package.handle)?)
            .map_err(|_| ElevatedHelperError::Rollback)?;
        rename_exact(
            &source,
            self.stage_root_handle
                .as_ref()
                .ok_or(ElevatedHelperError::Rollback)?,
            self.stage_package
                .file_name()
                .ok_or(ElevatedHelperError::Rollback)?,
            false,
        )
        .map_err(|_| ElevatedHelperError::Rollback)?;
        if node_identity(&source).map_err(|_| ElevatedHelperError::Rollback)?
            != node_identity(&package.handle).map_err(|_| ElevatedHelperError::Rollback)?
        {
            return Err(ElevatedHelperError::Rollback);
        }
        drop(proof);
        Ok(())
    }

    fn rollback_state(&mut self) -> Result<(), ElevatedHelperError> {
        let state = self
            .state_handle
            .as_ref()
            .ok_or(ElevatedHelperError::Rollback)?;
        if node_identity(state).map_err(|_| ElevatedHelperError::Rollback)?
            != self.state_identity.ok_or(ElevatedHelperError::Rollback)?
        {
            return Err(ElevatedHelperError::Rollback);
        }
        let effect_path = handle_path(state).map_err(|_| ElevatedHelperError::Rollback)?;
        if same_native_path(&effect_path, &self.stage_state) {
            return Ok(());
        }
        validate_provisioned_state_root(&effect_path).map_err(|_| ElevatedHelperError::Rollback)?;
        if !bounded_names(&effect_path, 0, ElevatedHelperError::Rollback)?.is_empty() {
            return Err(ElevatedHelperError::Rollback);
        }
        rename_exact(
            state,
            self.stage_root_handle
                .as_ref()
                .ok_or(ElevatedHelperError::Rollback)?,
            self.stage_state
                .file_name()
                .ok_or(ElevatedHelperError::Rollback)?,
            false,
        )
        .map_err(|_| ElevatedHelperError::Rollback)?;
        self.require_bound_state_at(&self.stage_state)
            .map_err(|_| ElevatedHelperError::Rollback)
    }

    fn hold_published_pair(&mut self) -> Result<(), ElevatedHelperError> {
        let package_root = ConfiguredManagedPackageRoot::new(self.package_target.clone())
            .map_err(|_| ElevatedHelperError::Drift)?;
        let package = inspect_managed_package(&self.packet, &package_root)
            .map_err(|_| ElevatedHelperError::Drift)?;
        validate_provisioned_package_acl(&package).map_err(|_| ElevatedHelperError::Drift)?;
        let package_node = package.root_node().ok_or(ElevatedHelperError::Drift)?;
        let package_source =
            open_rename_directory(&package_node.path, node_identity(&package_node.handle)?)?;
        let (state, state_identity) = open_mutation_directory(&self.state_target, None)?;
        validate_provisioned_state_root(&self.state_target)
            .map_err(|_| ElevatedHelperError::Drift)?;
        if !bounded_names(&self.state_target, 0, ElevatedHelperError::Drift)?.is_empty()
            || self.held_package.is_some()
            || self.held_state.is_some()
            || exact_directory_presence(&self.stage_root.join(HELD_PACKAGE_NAME))?
            || exact_directory_presence(&self.stage_root.join(HELD_STATE_NAME))?
        {
            return Err(ElevatedHelperError::Drift);
        }
        rename_exact(
            &package_source,
            self.stage_root_handle
                .as_ref()
                .ok_or(ElevatedHelperError::Drift)?,
            std::ffi::OsStr::new(HELD_PACKAGE_NAME),
            false,
        )?;
        if let Err(error) = rename_exact(
            &state,
            self.stage_root_handle
                .as_ref()
                .ok_or(ElevatedHelperError::Drift)?,
            std::ffi::OsStr::new(HELD_STATE_NAME),
            false,
        ) {
            let restored = rename_exact(
                &package_source,
                &self.package_parent_handle,
                self.package_target
                    .file_name()
                    .ok_or(ElevatedHelperError::Drift)?,
                false,
            );
            return restored.and(Err(error));
        }
        self.held_package = Some(package);
        self.held_state = Some(state);
        self.held_state_identity = Some(state_identity);
        Ok(())
    }

    fn discard_held_pair(&mut self) -> Result<(), ElevatedHelperError> {
        if self.held_package.is_some()
            || self.held_state.is_some()
            || exact_directory_presence(&self.stage_root.join(HELD_PACKAGE_NAME))?
            || exact_directory_presence(&self.stage_root.join(HELD_STATE_NAME))?
        {
            return Err(ElevatedHelperError::Cleanup);
        }
        Ok(())
    }

    fn restore_held_pair(&mut self) -> Result<(), ElevatedHelperError> {
        let package = self
            .held_package
            .as_ref()
            .ok_or(ElevatedHelperError::Rollback)?;
        let package_node = package.root_node().ok_or(ElevatedHelperError::Rollback)?;
        let package_source = open_rename_directory(
            &self.stage_root.join(HELD_PACKAGE_NAME),
            node_identity(&package_node.handle).map_err(|_| ElevatedHelperError::Rollback)?,
        )
        .map_err(|_| ElevatedHelperError::Rollback)?;
        let state = self
            .held_state
            .as_ref()
            .ok_or(ElevatedHelperError::Rollback)?;
        if node_identity(state).map_err(|_| ElevatedHelperError::Rollback)?
            != self
                .held_state_identity
                .ok_or(ElevatedHelperError::Rollback)?
        {
            return Err(ElevatedHelperError::Rollback);
        }
        rename_exact(
            &package_source,
            &self.package_parent_handle,
            self.package_target
                .file_name()
                .ok_or(ElevatedHelperError::Rollback)?,
            false,
        )
        .map_err(|_| ElevatedHelperError::Rollback)?;
        rename_exact(
            state,
            &self.state_parent_handle,
            self.state_target
                .file_name()
                .ok_or(ElevatedHelperError::Rollback)?,
            false,
        )
        .map_err(|_| ElevatedHelperError::Rollback)?;
        self.held_package.take();
        self.held_state.take();
        self.held_state_identity.take();
        Ok(())
    }

    fn retire_published_package(&mut self) -> Result<(), ElevatedHelperError> {
        let package_root = ConfiguredManagedPackageRoot::new(self.package_target.clone())
            .map_err(|_| ElevatedHelperError::Drift)?;
        let package = inspect_managed_package(&self.packet, &package_root)
            .map_err(|_| ElevatedHelperError::Drift)?;
        validate_provisioned_package_acl(&package).map_err(|_| ElevatedHelperError::Drift)?;
        validate_provisioned_state_root(&self.state_target)
            .map_err(|_| ElevatedHelperError::Drift)?;
        if !bounded_names(&self.state_target, 0, ElevatedHelperError::Drift)?.is_empty() {
            return Err(ElevatedHelperError::Drift);
        }
        drop(package);
        remove_exact_directory_tree(&self.package_target)
    }

    fn retire_published_state(&mut self) -> Result<(), ElevatedHelperError> {
        self.require_bound_state_at(&self.state_target)?;
        validate_provisioned_state_root(&self.state_target)
            .map_err(|_| ElevatedHelperError::Drift)?;
        if !bounded_names(&self.state_target, 0, ElevatedHelperError::Drift)?.is_empty() {
            return Err(ElevatedHelperError::Drift);
        }
        remove_exact_directory_tree(&self.state_target)
    }
}

impl WindowsPlatform {
    fn require_stage_root_at_path(&self) -> Result<(), ElevatedHelperError> {
        let held = self
            .stage_root_handle
            .as_ref()
            .ok_or(ElevatedHelperError::Drift)?;
        let reopened = open_destination_root(&self.stage_root)?;
        if node_identity(held)? != node_identity(&reopened)? {
            return Err(ElevatedHelperError::Drift);
        }
        Ok(())
    }

    fn require_bound_state_at(&self, path: &Path) -> Result<(), ElevatedHelperError> {
        let expected = self.state_identity.ok_or(ElevatedHelperError::Drift)?;
        let held = self
            .state_handle
            .as_ref()
            .ok_or(ElevatedHelperError::Drift)?;
        if node_identity(held)? != expected {
            return Err(ElevatedHelperError::Drift);
        }
        open_mutation_directory(path, Some(expected))?;
        Ok(())
    }
}
