package com.gsplat.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class GsplatSurfaceCurrentStatsTest {
    @Test
    fun mapsEveryExplicitRequestAndPollStatus() {
        GsplatSurfaceCurrentStatsRequestStatus.entries.forEach { status ->
            assertEquals(
                status,
                GsplatSurfaceCurrentStatsRequest.fromRaw(
                    longArrayOf(status.nativeValue.toLong())
                ).status
            )
        }
        assertEquals(
            GsplatSurfaceCurrentStatsPollKind.EMPTY,
            GsplatSurfaceCurrentStatsPoll.fromRaw(emptyPollRaw()).kind
        )
        GsplatSurfaceCurrentStatsRequestStatus.entries
            .filterNot { it == GsplatSurfaceCurrentStatsRequestStatus.REQUESTED }
            .forEach { status ->
                val raw = emptyPollRaw().also {
                    it[0] = GsplatSurfaceCurrentStatsPollKind.UNSAMPLED.nativeValue.toLong()
                    it[1] = status.nativeValue.toLong()
                }
                assertEquals(status, GsplatSurfaceCurrentStatsPoll.fromRaw(raw).requestStatus)
            }
    }

    @Test
    fun completeIdentityIncludesEveryJoinField() {
        val identity = identity()
        val raw = LongArray(GsplatSurfaceCurrentStatsIdentity.RAW_VALUE_COUNT)
        identity.writeRaw(raw, 0)
        assertEquals(identity, GsplatSurfaceCurrentStatsIdentity.fromRaw(raw, 0))

        val variants = listOf(
            identity.copy(sceneGeneration = 100),
            identity.copy(cameraRevision = 100),
            identity.copy(viewportGeneration = 100),
            identity.copy(contractGeneration = 100),
            identity.copy(planSetGeneration = 100),
            identity.copy(orderGeneration = 100),
            identity.copy(rasterGeneration = 100),
            identity.copy(encodeAttempt = 100),
            identity.copy(presentationSequence = 100),
            identity.copy(executedPlan = GsplatSurfaceCurrentStatsPlan.GPU_PREPROJECT)
        )

        assertTrue(variants.all { it != identity })
    }

    @Test
    fun mapsNotRequestedAndIssuedSubmissionPayloads() {
        val notRequestedRaw = LongArray(GsplatSurfaceCurrentStatsSubmission.RAW_VALUE_COUNT)
        notRequestedRaw[0] =
            GsplatSurfaceCurrentStatsSubmissionStatus.NOT_REQUESTED.nativeValue.toLong()
        assertEquals(notRequested(), GsplatSurfaceCurrentStatsSubmission.fromRaw(notRequestedRaw))

        val identity = identity()
        val issuedRaw = LongArray(GsplatSurfaceCurrentStatsSubmission.RAW_VALUE_COUNT)
        issuedRaw[0] = GsplatSurfaceCurrentStatsSubmissionStatus.ISSUED.nativeValue.toLong()
        issuedRaw[1] = 37
        identity.writeRaw(issuedRaw, 2)
        assertEquals(issued(37, identity), GsplatSurfaceCurrentStatsSubmission.fromRaw(issuedRaw))
    }

    @Test
    fun issuedThenEmptyIsPendingWithoutCounts() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val cycle = adapter.consume(
            requested(),
            issued(ticket = 41, identity = identity()),
            emptyPoll()
        )

        assertTrue(cycle.state is GsplatSurfaceCurrentStatsState.Pending)
        assertEquals(1, adapter.pendingCount)
        assertNull(adapter.currentReceipt)
    }

    @Test
    fun requestedWithoutSubmissionIsExplicitlyAwaitingAcrossFrames() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val cycle = adapter.consume(requested(), notRequested(), emptyPoll())

        assertTrue(cycle.state is GsplatSurfaceCurrentStatsState.AwaitingSubmission)
        assertTrue(adapter.requiresSubmissionReconciliation)
        assertEquals(0, adapter.pendingCount)
        assertNull(adapter.currentReceipt)
    }

    @Test
    fun issuedThenEmptyCanCompleteThroughIndependentPoll() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val identity = identity()
        adapter.consume(requested(), issued(ticket = 43, identity = identity), emptyPoll())

        val state = adapter.consumePoll(
            readyPoll(
                ticket = 43,
                identity = identity,
                source = 100,
                visible = 80,
                contributor = 55,
                drawn = 55,
                semantics =
                    GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_CONTRIBUTOR
            )
        )

        val ready = state as GsplatSurfaceCurrentStatsState.Ready
        assertEquals(43L, ready.receipt.ticket)
        assertEquals(0, adapter.pendingCount)
        assertEquals(ready.receipt, adapter.currentReceipt)
    }

    @Test
    fun failedFrameIntentCanLandAsBusyThenIssuedOnNextSuccessfulFrame() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        adapter.consume(requested(), notRequested(), emptyPoll())

        val cycle = adapter.consume(
            request(GsplatSurfaceCurrentStatsRequestStatus.BUSY),
            issued(ticket = 45, identity = identity()),
            emptyPoll()
        )

        assertTrue(cycle.state is GsplatSurfaceCurrentStatsState.Pending)
        assertEquals(45L, (cycle.state as GsplatSurfaceCurrentStatsState.Pending).ticket)
        assertEquals(1, adapter.pendingCount)
    }

    @Test
    fun busyAndLegacyUnavailableAreNonFatalAndNeverInventCounts() {
        val statuses = listOf(
            GsplatSurfaceCurrentStatsRequestStatus.BUSY,
            GsplatSurfaceCurrentStatsRequestStatus.GPU_UNAVAILABLE,
            GsplatSurfaceCurrentStatsRequestStatus.RESOURCE_UNAVAILABLE,
            GsplatSurfaceCurrentStatsRequestStatus.TICKET_EXHAUSTED
        )
        statuses.forEach { status ->
            val adapter = GsplatSurfaceCurrentStatsAdapter()
            val cycle = adapter.consume(request(status), notRequested(), emptyPoll())
            val unavailable = cycle.state as GsplatSurfaceCurrentStatsState.Unavailable
            assertEquals(status, unavailable.status)
            assertNull(adapter.currentReceipt)
            assertEquals(0, adapter.pendingCount)
        }
    }

    @Test
    fun matchingReadyPublishesAllCountsAtomically() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val identity = identity()
        adapter.consume(requested(), issued(51, identity), emptyPoll())

        val cycle = adapter.consume(
            request(GsplatSurfaceCurrentStatsRequestStatus.BUSY),
            notRequested(),
            readyPoll(
                ticket = 51,
                identity = identity,
                source = 100,
                visible = 80,
                contributor = 55,
                drawn = 55,
                semantics =
                    GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_CONTRIBUTOR
            )
        )

        val ready = cycle.state as GsplatSurfaceCurrentStatsState.Ready
        assertEquals(100, ready.receipt.sourceCount)
        assertEquals(80, ready.receipt.visibleCount)
        assertEquals(55, ready.receipt.contributorCount)
        assertEquals(55, ready.receipt.drawnCount)
        assertEquals(0, adapter.pendingCount)
        assertEquals(ready.receipt, adapter.currentReceipt)
    }

    @Test
    fun rereadingIssuedAfterReadyDoesNotRecreatePending() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val identity = identity()
        val submission = issued(53, identity)
        adapter.consume(requested(), submission, emptyPoll())
        adapter.consumePoll(
            readyPoll(
                ticket = 53,
                identity = identity,
                source = 100,
                visible = 80,
                contributor = 55,
                drawn = 80,
                semantics =
                    GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_VISIBLE
            )
        )

        val reread = adapter.consume(
            request(GsplatSurfaceCurrentStatsRequestStatus.BUSY),
            submission,
            emptyPoll()
        )

        assertFalse(reread.state is GsplatSurfaceCurrentStatsState.Pending)
        assertEquals(0, adapter.pendingCount)
        assertNull(adapter.currentReceipt)
    }

    @Test
    fun terminalFailureAndExpiryClearOnlyMatchingPending() {
        GsplatSurfaceCurrentStatsPollKind.entries
            .filter { it.isTerminalFailure }
            .forEachIndexed { index, kind ->
                val adapter = GsplatSurfaceCurrentStatsAdapter()
                val ticket = 60L + index
                val identity = identity().copy(encodeAttempt = ticket)
                adapter.consume(requested(), issued(ticket, identity), emptyPoll())

                val failed = adapter.consume(
                    request(GsplatSurfaceCurrentStatsRequestStatus.BUSY),
                    notRequested(),
                    failurePoll(kind, ticket, identity)
                ).state as GsplatSurfaceCurrentStatsState.Failed

                assertEquals(kind, failed.failure.kind)
                assertEquals(0, adapter.pendingCount)
                assertNull(adapter.currentReceipt)
            }
    }

    @Test
    fun rereadingIssuedAfterEveryFailureDoesNotRecreatePending() {
        GsplatSurfaceCurrentStatsPollKind.entries
            .filter { it.isTerminalFailure }
            .forEachIndexed { index, kind ->
                val adapter = GsplatSurfaceCurrentStatsAdapter()
                val ticket = 100L + index
                val identity = identity().copy(encodeAttempt = ticket)
                val submission = issued(ticket, identity)
                adapter.consume(requested(), submission, emptyPoll())
                adapter.consumePoll(failurePoll(kind, ticket, identity))

                val reread = adapter.consume(
                    request(GsplatSurfaceCurrentStatsRequestStatus.BUSY),
                    submission,
                    emptyPoll()
                )

                assertFalse(reread.state is GsplatSurfaceCurrentStatsState.Pending)
                assertEquals(0, adapter.pendingCount)
                assertNull(adapter.currentReceipt)
            }
    }

    @Test
    fun ticketMatchWithoutFullIdentityIsRejected() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val identity = identity()
        adapter.consume(requested(), issued(71, identity), emptyPoll())

        val rejected = adapter.consume(
            request(GsplatSurfaceCurrentStatsRequestStatus.BUSY),
            notRequested(),
            readyPoll(
                ticket = 71,
                identity = identity.copy(contractGeneration = 999),
                source = 100,
                visible = 80,
                contributor = 55,
                drawn = 80,
                semantics = GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_VISIBLE
            )
        ).state as GsplatSurfaceCurrentStatsState.Rejected

        assertEquals("ready_identity_mismatch", rejected.reason)
        assertEquals(0, adapter.pendingCount)
        assertNull(adapter.currentReceipt)
    }

    @Test
    fun mismatchedFailureAlsoConsumesAndClearsPendingTicket() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val identity = identity()
        adapter.consume(requested(), issued(75, identity), emptyPoll())

        val rejected = adapter.consume(
            request(GsplatSurfaceCurrentStatsRequestStatus.BUSY),
            notRequested(),
            failurePoll(
                GsplatSurfaceCurrentStatsPollKind.EXPIRED,
                75,
                identity.copy(rasterGeneration = 999)
            )
        ).state as GsplatSurfaceCurrentStatsState.Rejected

        assertEquals("failure_identity_mismatch", rejected.reason)
        assertEquals(0, adapter.pendingCount)
        assertNull(adapter.currentReceipt)
    }

    @Test
    fun submissionIdentityDriftStillAccountsForAlreadyPoppedTerminal() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val identity = identity()
        adapter.consume(requested(), issued(77, identity), emptyPoll())

        val cycle = adapter.consume(
            request(GsplatSurfaceCurrentStatsRequestStatus.BUSY),
            issued(77, identity.copy(planSetGeneration = 999)),
            readyPoll(
                ticket = 77,
                identity = identity,
                source = 100,
                visible = 80,
                contributor = 55,
                drawn = 80,
                semantics =
                    GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_VISIBLE
            )
        )

        val rejected = cycle.state as GsplatSurfaceCurrentStatsState.Rejected
        assertEquals("submission_ticket_identity_mismatch", rejected.reason)
        assertEquals(0, adapter.pendingCount)
        assertNull(adapter.currentReceipt)
    }

    @Test
    fun multiplePendingTicketsAcceptOutOfOrderReadyAndFailureTerminals() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val identities = (1L..3L).associateWith { ticket ->
            identity().copy(encodeAttempt = ticket)
        }
        identities.forEach { (ticket, identity) ->
            adapter.consume(requested(), issued(ticket, identity), emptyPoll())
        }
        assertEquals(3, adapter.pendingCount)

        val third = adapter.consumePoll(
            readyPoll(
                ticket = 3,
                identity = checkNotNull(identities[3]),
                source = 10,
                visible = 8,
                contributor = 6,
                drawn = 6,
                semantics =
                    GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_CONTRIBUTOR
            )
        ) as GsplatSurfaceCurrentStatsState.Ready
        assertEquals(3, third.receipt.ticket)
        assertEquals(2, adapter.pendingCount)

        val first = adapter.consumePoll(
            failurePoll(
                GsplatSurfaceCurrentStatsPollKind.MAP_FAILURE,
                1,
                checkNotNull(identities[1])
            )
        ) as GsplatSurfaceCurrentStatsState.Failed
        assertEquals(1, first.failure.ticket)
        assertEquals(1, adapter.pendingCount)

        val second = adapter.consumePoll(
            readyPoll(
                ticket = 2,
                identity = checkNotNull(identities[2]),
                source = 10,
                visible = 8,
                contributor = 6,
                drawn = 8,
                semantics =
                    GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_VISIBLE
            )
        ) as GsplatSurfaceCurrentStatsState.Ready
        assertEquals(2, second.receipt.ticket)
        assertEquals(0, adapter.pendingCount)
    }

    @Test
    fun completedTicketTombstonesRemainBoundedAcrossLongSequences() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        (1L..24L).forEach { ticket ->
            val identity = identity().copy(encodeAttempt = ticket)
            val submission = issued(ticket, identity)
            adapter.consume(requested(), submission, emptyPoll())
            adapter.consumePoll(
                readyPoll(
                    ticket = ticket,
                    identity = identity,
                    source = 1,
                    visible = 1,
                    contributor = 1,
                    drawn = 1,
                    semantics =
                        GsplatSurfaceCurrentStatsCountSemantics.DIRECT_DRAW_EQUALS_VISIBLE
                )
            )
        }

        assertEquals(0, adapter.pendingCount)
        assertEquals(8, adapter.trackedTombstoneCount)

        val latestIdentity = identity().copy(encodeAttempt = 24)
        adapter.consume(
            request(GsplatSurfaceCurrentStatsRequestStatus.BUSY),
            issued(24, latestIdentity),
            emptyPoll()
        )
        assertEquals(0, adapter.pendingCount)
        assertEquals(8, adapter.trackedTombstoneCount)
    }

    @Test
    fun failedObserverFrameThenOrdinarySuccessReconcilesRetainedIntent() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val request = adapter.observeRequest(requested())
        adapter.abandonFrame()
        assertTrue(adapter.state is GsplatSurfaceCurrentStatsState.AwaitingSubmission)
        assertTrue(adapter.requiresSubmissionReconciliation)

        var submissionReads = 0
        var pollReads = 0
        val firstOrdinaryFrame = adapter.reconcileAfterSuccessfulRender(
            readSubmission = {
                submissionReads += 1
                notRequested()
            },
            readPoll = {
                pollReads += 1
                emptyPoll()
            }
        )
        assertEquals(request, firstOrdinaryFrame?.request)
        assertTrue(firstOrdinaryFrame?.state is GsplatSurfaceCurrentStatsState.AwaitingSubmission)
        assertTrue(adapter.requiresSubmissionReconciliation)

        val identity = identity()
        val secondOrdinaryFrame = adapter.reconcileAfterSuccessfulRender(
            readSubmission = {
                submissionReads += 1
                issued(79, identity)
            },
            readPoll = {
                pollReads += 1
                emptyPoll()
            }
        )
        assertTrue(secondOrdinaryFrame?.state is GsplatSurfaceCurrentStatsState.Pending)
        assertFalse(adapter.requiresSubmissionReconciliation)
        assertEquals(1, adapter.pendingCount)
        assertEquals(2, submissionReads)
        assertEquals(2, pollReads)

        val terminal = adapter.consumePoll(
            readyPoll(
                ticket = 79,
                identity = identity,
                source = 10,
                visible = 8,
                contributor = 6,
                drawn = 8,
                semantics =
                    GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_VISIBLE
            )
        ) as GsplatSurfaceCurrentStatsState.Ready
        assertEquals(79L, terminal.receipt.ticket)
        assertEquals(0, adapter.pendingCount)

        assertNull(
            adapter.reconcileAfterSuccessfulRender(
                readSubmission = { error("ordinary frame must not read submission") },
                readPoll = { error("ordinary frame must not poll") }
            )
        )
    }

    @Test
    fun olderTicketTerminalDoesNotClearNewerPreTicketIntent() {
        listOf(
            GsplatSurfaceCurrentStatsPollKind.READY,
            GsplatSurfaceCurrentStatsPollKind.MAP_FAILURE
        ).forEachIndexed { index, terminalKind ->
            val adapter = GsplatSurfaceCurrentStatsAdapter()
            val firstIdentity = identity().copy(encodeAttempt = 200L + index)
            adapter.consume(
                requested(),
                issued(201L + index, firstIdentity),
                emptyPoll()
            )
            assertEquals(1, adapter.pendingCount)

            adapter.observeRequest(requested())
            adapter.abandonFrame()
            assertTrue(adapter.requiresSubmissionReconciliation)

            val terminal = if (terminalKind == GsplatSurfaceCurrentStatsPollKind.READY) {
                readyPoll(
                    ticket = 201L + index,
                    identity = firstIdentity,
                    source = 10,
                    visible = 8,
                    contributor = 6,
                    drawn = 8,
                    semantics =
                        GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_VISIBLE
                )
            } else {
                failurePoll(terminalKind, 201L + index, firstIdentity)
            }
            adapter.consumePoll(terminal)

            assertEquals(0, adapter.pendingCount)
            assertTrue(adapter.requiresSubmissionReconciliation)

            val secondIdentity = identity().copy(encodeAttempt = 300L + index)
            val recovered = adapter.reconcileAfterSuccessfulRender(
                readSubmission = { issued(301L + index, secondIdentity) },
                readPoll = { emptyPoll() }
            )
            val pending = recovered?.state as GsplatSurfaceCurrentStatsState.Pending
            assertEquals(301L + index, pending.ticket)
            assertEquals(1, adapter.pendingCount)
            assertFalse(adapter.requiresSubmissionReconciliation)
        }
    }

    @Test
    fun ordinaryReconciliationConsumesOlderReadyWithoutPublishingItsCounts() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val olderIdentity = identity().copy(
            encodeAttempt = 310,
            presentationSequence = 310
        )
        adapter.consume(requested(), issued(310, olderIdentity), emptyPoll())
        adapter.observeRequest(requested())
        adapter.abandonFrame()

        val currentIdentity = identity().copy(
            encodeAttempt = 311,
            presentationSequence = 311
        )
        val olderReady = readyPoll(
            ticket = 310,
            identity = olderIdentity,
            source = 10,
            visible = 8,
            contributor = 6,
            drawn = 8,
            semantics = GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_VISIBLE
        )
        val cycle = checkNotNull(
            adapter.reconcileAfterOrdinaryRender(
                readSubmission = { issued(311, currentIdentity) },
                readPoll = { olderReady }
            )
        )

        assertEquals(olderReady, cycle.poll)
        val pending = cycle.state as GsplatSurfaceCurrentStatsState.Pending
        assertEquals(311L, pending.ticket)
        assertEquals(currentIdentity, pending.identity)
        assertEquals(1, pending.pendingCount)
        assertNull(adapter.currentReceipt)
        assertEquals(1, adapter.trackedTombstoneCount)
        assertFalse(adapter.requiresSubmissionReconciliation)

        val currentReady = adapter.consumePoll(
            readyPoll(
                ticket = 311,
                identity = currentIdentity,
                source = 10,
                visible = 7,
                contributor = 5,
                drawn = 7,
                semantics = GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_VISIBLE
            )
        ) as GsplatSurfaceCurrentStatsState.Ready
        assertEquals(311L, currentReady.receipt.ticket)
        assertEquals(0, adapter.pendingCount)
        assertEquals(2, adapter.trackedTombstoneCount)
    }

    @Test
    fun explicitPresentationConsumesOlderReadyWithoutPublishingItsCounts() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val olderIdentity = identity().copy(
            encodeAttempt = 312,
            presentationSequence = 312
        )
        adapter.consume(requested(), issued(312, olderIdentity), emptyPoll())

        val currentRequest = adapter.observeRequest(requested())
        val currentIdentity = identity().copy(
            encodeAttempt = 313,
            presentationSequence = 313
        )
        val olderReady = readyPoll(
            ticket = 312,
            identity = olderIdentity,
            source = 10,
            visible = 8,
            contributor = 6,
            drawn = 8,
            semantics = GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_VISIBLE
        )
        val cycle = adapter.completeAfterSuccessfulPresentation(
            request = currentRequest,
            readSubmission = { issued(313, currentIdentity) },
            readPoll = { olderReady }
        )

        assertEquals(olderReady, cycle.poll)
        val pending = cycle.state as GsplatSurfaceCurrentStatsState.Pending
        assertEquals(313L, pending.ticket)
        assertEquals(currentIdentity, pending.identity)
        assertEquals(1, pending.pendingCount)
        assertNull(adapter.currentReceipt)
        assertEquals(1, adapter.trackedTombstoneCount)

        val currentReady = adapter.consumePoll(
            readyPoll(
                ticket = 313,
                identity = currentIdentity,
                source = 10,
                visible = 7,
                contributor = 5,
                drawn = 7,
                semantics = GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_VISIBLE
            )
        ) as GsplatSurfaceCurrentStatsState.Ready
        assertEquals(313L, currentReady.receipt.ticket)
        assertEquals(currentReady.receipt, adapter.currentReceipt)
        assertEquals(0, adapter.pendingCount)
        assertEquals(2, adapter.trackedTombstoneCount)
    }

    @Test
    fun ordinaryReconciliationKeepsAwaitingWhenOlderReadyPrecedesSubmission() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val olderIdentity = identity().copy(
            encodeAttempt = 320,
            presentationSequence = 320
        )
        adapter.consume(requested(), issued(320, olderIdentity), emptyPoll())
        adapter.observeRequest(requested())
        adapter.abandonFrame()

        val olderReady = readyPoll(
            ticket = 320,
            identity = olderIdentity,
            source = 10,
            visible = 8,
            contributor = 6,
            drawn = 8,
            semantics = GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_VISIBLE
        )
        val cycle = checkNotNull(
            adapter.reconcileAfterOrdinaryRender(
                readSubmission = { notRequested() },
                readPoll = { olderReady }
            )
        )

        assertEquals(olderReady, cycle.poll)
        assertTrue(cycle.state is GsplatSurfaceCurrentStatsState.AwaitingSubmission)
        assertNull(adapter.currentReceipt)
        assertEquals(0, adapter.pendingCount)
        assertEquals(1, adapter.trackedTombstoneCount)
        assertTrue(adapter.requiresSubmissionReconciliation)
    }

    @Test
    fun recoveryReadFailureStaysFailClosedAndRetryable() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        adapter.observeRequest(requested())
        adapter.abandonFrame()

        val ordinaryRenderResult = adapter.reconcileAfterOrdinaryRender(
            readSubmission = { error("synthetic submission read failure") },
            readPoll = { error("poll must not run") }
        )

        assertNull(ordinaryRenderResult)
        val rejected = adapter.state as GsplatSurfaceCurrentStatsState.Rejected
        assertEquals("submission_reconciliation_failed", rejected.reason)
        assertTrue(adapter.requiresSubmissionReconciliation)
        assertNull(adapter.currentReceipt)

        val recovered = adapter.reconcileAfterSuccessfulRender(
            readSubmission = { notRequested() },
            readPoll = {
                GsplatSurfaceCurrentStatsPoll(
                    kind = GsplatSurfaceCurrentStatsPollKind.UNSAMPLED,
                    requestStatus =
                        GsplatSurfaceCurrentStatsRequestStatus.RESOURCE_UNAVAILABLE
                )
            }
        )
        assertTrue(recovered?.state is GsplatSurfaceCurrentStatsState.Unavailable)
        assertFalse(adapter.requiresSubmissionReconciliation)
        assertNull(adapter.currentReceipt)
    }

    @Test
    fun recoveryPollFailureAfterIssuedKeepsKnownPendingForExplicitPoll() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val identity = identity().copy(encodeAttempt = 401)
        adapter.observeRequest(requested())
        adapter.abandonFrame()

        val ordinaryRenderResult = adapter.reconcileAfterOrdinaryRender(
            readSubmission = { issued(401, identity) },
            readPoll = { error("synthetic poll parse failure") }
        )

        assertNull(ordinaryRenderResult)
        assertFalse(adapter.requiresSubmissionReconciliation)
        assertEquals(1, adapter.pendingCount)
        assertTrue(adapter.state is GsplatSurfaceCurrentStatsState.Rejected)
        assertNull(adapter.currentReceipt)

        val ready = adapter.consumePoll(
            readyPoll(
                ticket = 401,
                identity = identity,
                source = 10,
                visible = 8,
                contributor = 6,
                drawn = 8,
                semantics =
                    GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_VISIBLE
            )
        ) as GsplatSurfaceCurrentStatsState.Ready
        assertEquals(401L, ready.receipt.ticket)
        assertEquals(0, adapter.pendingCount)
    }

    @Test
    fun fullIdentityAllowsOpaqueZeroValues() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val zeroIdentity = GsplatSurfaceCurrentStatsIdentity(
            sceneGeneration = 0,
            cameraRevision = 0,
            viewportGeneration = 0,
            contractGeneration = 0,
            planSetGeneration = 0,
            orderGeneration = 0,
            rasterGeneration = 0,
            encodeAttempt = 0,
            presentationSequence = 0,
            executedPlan = GsplatSurfaceCurrentStatsPlan.CPU_POST_SORT
        )
        adapter.consume(requested(), issued(83, zeroIdentity), emptyPoll())

        val ready = adapter.consumePoll(
            readyPoll(
                ticket = 83,
                identity = zeroIdentity,
                source = 0,
                visible = 0,
                contributor = 0,
                drawn = 0,
                semantics = GsplatSurfaceCurrentStatsCountSemantics.DIRECT_DRAW_EQUALS_VISIBLE
            )
        ) as GsplatSurfaceCurrentStatsState.Ready

        assertEquals(zeroIdentity, ready.receipt.identity)
        assertEquals(0, adapter.pendingCount)
    }

    @Test
    fun unavailableCycleClearsPreviouslyPublishedReadyInsteadOfFallingBack() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val identity = identity()
        adapter.consume(requested(), issued(81, identity), emptyPoll())
        adapter.consume(
            request(GsplatSurfaceCurrentStatsRequestStatus.BUSY),
            notRequested(),
            readyPoll(
                ticket = 81,
                identity = identity,
                source = 10,
                visible = 8,
                contributor = 6,
                drawn = 8,
                semantics = GsplatSurfaceCurrentStatsCountSemantics.DIRECT_DRAW_EQUALS_VISIBLE
            )
        )
        assertTrue(adapter.currentReceipt != null)

        adapter.consume(
            request(GsplatSurfaceCurrentStatsRequestStatus.GPU_UNAVAILABLE),
            notRequested(),
            emptyPoll()
        )

        assertNull(adapter.currentReceipt)
        assertTrue(adapter.state is GsplatSurfaceCurrentStatsState.Unavailable)
    }

    @Test
    fun ordinaryRenderInvalidatesPreviouslyPublishedReadySnapshot() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val identity = identity()
        adapter.consume(
            requested(),
            issued(85, identity),
            readyPoll(
                ticket = 85,
                identity = identity,
                source = 10,
                visible = 8,
                contributor = 6,
                drawn = 8,
                semantics =
                    GsplatSurfaceCurrentStatsCountSemantics.DIRECT_DRAW_EQUALS_VISIBLE
            )
        )
        assertNotNull(adapter.currentReceipt)

        adapter.observeOrdinaryRender()

        assertNull(adapter.currentReceipt)
        assertTrue(adapter.state is GsplatSurfaceCurrentStatsState.NotRequested)
    }

    @Test
    fun ordinaryRenderKeepsAnIssuedTicketPendingWithoutExposingCounts() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val identity = identity()
        adapter.consume(requested(), issued(87, identity), emptyPoll())

        adapter.observeOrdinaryRender()

        val pending = adapter.state as GsplatSurfaceCurrentStatsState.Pending
        assertEquals(87L, pending.ticket)
        assertEquals(identity, pending.identity)
        assertNull(adapter.currentReceipt)
    }

    @Test
    fun readyPolledAfterANewerOrdinaryPresentationStaysHistorical() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val historicalIdentity = identity().copy(
            encodeAttempt = 88,
            presentationSequence = 88
        )
        adapter.consume(requested(), issued(88, historicalIdentity), emptyPoll())

        adapter.observeOrdinaryRender()
        val historicalState = adapter.consumePoll(
            readyPoll(
                ticket = 88,
                identity = historicalIdentity,
                source = 10,
                visible = 8,
                contributor = 6,
                drawn = 8,
                semantics = GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_VISIBLE
            )
        )

        assertTrue(historicalState is GsplatSurfaceCurrentStatsState.NotRequested)
        assertNull(adapter.currentReceipt)
        assertEquals(0, adapter.pendingCount)
        assertEquals(1, adapter.trackedTombstoneCount)

        val currentIdentity = identity().copy(
            encodeAttempt = 89,
            presentationSequence = 89
        )
        adapter.consume(requested(), issued(89, currentIdentity), emptyPoll())
        val currentState = adapter.consumePoll(
            readyPoll(
                ticket = 89,
                identity = currentIdentity,
                source = 10,
                visible = 7,
                contributor = 5,
                drawn = 7,
                semantics = GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_VISIBLE
            )
        ) as GsplatSurfaceCurrentStatsState.Ready

        assertEquals(89L, currentState.receipt.ticket)
        assertEquals(currentState.receipt, adapter.currentReceipt)
        assertEquals(2, adapter.trackedTombstoneCount)
    }

    @Test
    fun readyParserRejectsZeroTicketAndInvalidCountSemantics() {
        val zeroTicket = readyPollRaw(
            ticket = 0,
            identity = identity(),
            source = 10,
            visible = 8,
            contributor = 6,
            drawn = 8,
            semantics = GsplatSurfaceCurrentStatsCountSemantics.DIRECT_DRAW_EQUALS_VISIBLE
        )
        assertThrows(IllegalStateException::class.java) {
            GsplatSurfaceCurrentStatsPoll.fromRaw(zeroTicket)
        }

        val invalidDrawn = readyPollRaw(
            ticket = 91,
            identity = identity(),
            source = 10,
            visible = 8,
            contributor = 6,
            drawn = 7,
            semantics = GsplatSurfaceCurrentStatsCountSemantics.DIRECT_DRAW_EQUALS_VISIBLE
        )
        assertThrows(IllegalStateException::class.java) {
            GsplatSurfaceCurrentStatsPoll.fromRaw(invalidDrawn)
        }
    }

    private fun identity() = GsplatSurfaceCurrentStatsIdentity(
        sceneGeneration = 1,
        cameraRevision = 2,
        viewportGeneration = 3,
        contractGeneration = 4,
        planSetGeneration = 5,
        orderGeneration = 6,
        rasterGeneration = 7,
        encodeAttempt = 8,
        presentationSequence = 9,
        executedPlan = GsplatSurfaceCurrentStatsPlan.GPU_POST_SORT
    )

    private fun request(status: GsplatSurfaceCurrentStatsRequestStatus) =
        GsplatSurfaceCurrentStatsRequest(status)

    private fun requested() = request(GsplatSurfaceCurrentStatsRequestStatus.REQUESTED)

    private fun issued(ticket: Long, identity: GsplatSurfaceCurrentStatsIdentity) =
        GsplatSurfaceCurrentStatsSubmission(
            GsplatSurfaceCurrentStatsSubmissionStatus.ISSUED,
            ticket,
            identity
        )

    private fun notRequested() = GsplatSurfaceCurrentStatsSubmission(
        GsplatSurfaceCurrentStatsSubmissionStatus.NOT_REQUESTED,
        null,
        null
    )

    private fun emptyPoll() = GsplatSurfaceCurrentStatsPoll(
        GsplatSurfaceCurrentStatsPollKind.EMPTY
    )

    private fun readyPoll(
        ticket: Long,
        identity: GsplatSurfaceCurrentStatsIdentity,
        source: Long,
        visible: Long,
        contributor: Long,
        drawn: Long,
        semantics: GsplatSurfaceCurrentStatsCountSemantics
    ) = GsplatSurfaceCurrentStatsPoll.fromRaw(
        readyPollRaw(ticket, identity, source, visible, contributor, drawn, semantics)
    )

    private fun failurePoll(
        kind: GsplatSurfaceCurrentStatsPollKind,
        ticket: Long,
        identity: GsplatSurfaceCurrentStatsIdentity
    ): GsplatSurfaceCurrentStatsPoll {
        val raw = emptyPollRaw()
        raw[0] = kind.nativeValue.toLong()
        raw[3] = ticket
        identity.writeRaw(raw, 4)
        return GsplatSurfaceCurrentStatsPoll.fromRaw(raw)
    }

    private fun emptyPollRaw() = LongArray(GsplatSurfaceCurrentStatsPoll.RAW_VALUE_COUNT).also {
        it[0] = GsplatSurfaceCurrentStatsPollKind.EMPTY.nativeValue.toLong()
    }

    private fun readyPollRaw(
        ticket: Long,
        identity: GsplatSurfaceCurrentStatsIdentity,
        source: Long,
        visible: Long,
        contributor: Long,
        drawn: Long,
        semantics: GsplatSurfaceCurrentStatsCountSemantics
    ) = emptyPollRaw().also {
        it[0] = GsplatSurfaceCurrentStatsPollKind.READY.nativeValue.toLong()
        it[2] = semantics.nativeValue.toLong()
        it[3] = ticket
        identity.writeRaw(it, 4)
        it[14] = source
        it[15] = visible
        it[16] = contributor
        it[17] = drawn
    }
}
