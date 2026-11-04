import Foundation

extension Store {
    func configureNativeOwnership() {
        nativeOwnership.changed = { [weak self] in
            guard let self else { return }
            self.cancelHostExecution()
            self.readInputBoundary()
            self.status = self.nativeOwnership.summary
            if !self.nativeOwnership.allowsExecution {
                let epoch = self.nativeOwnership.generation
                Task { @MainActor [weak self] in
                    guard let self, self.nativeOwnership.generation == epoch,
                          !self.nativeOwnership.allowsExecution else { return }
                    await self.endLightingPreview()
                }
            }
        }
        nativeOwnership.drain = { [weak self] in
            guard let self else { return false }
            return await self.drainForNativeOwnership()
        }
        nativeOwnership.start()
    }

    func drainForNativeOwnership() async -> Bool {
        cancelHostExecution()
        readInputBoundary()
        // A preview can be sending before begin returns its token. Wait for that
        // operation too; stopping only the currently visible token is insufficient.
        let deadline = ContinuousClock.now.advanced(by: .seconds(2))
        while startingLightingPreview || endingLightingPreview || deviceOperationRunning || inFlightHostActions > 0 || pushToTalk.isActive {
            guard !Task.isCancelled, ContinuousClock.now < deadline else { return false }
            do { try await Task.sleep(for: .milliseconds(10)) } catch { return false }
        }
        await endLightingPreview()
        return !startingLightingPreview && !endingLightingPreview && lightingPreviewToken == nil && lightingCleanupConfirmed
            && !deviceOperationRunning && inFlightHostActions == 0 && pushToTalk.cleanupConfirmed && !pushToTalk.isActive
    }
}
