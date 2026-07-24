package com.gsplat.example

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SurfaceRenderTransactionTest {
    @Test
    fun failedCameraCommandDoesNotRequestOrRender() {
        val calls = ArrayList<String>()

        val result = performSurfaceRenderTransaction(
            applyCommand = {
                calls += "command"
                17
            },
            requestCurrentStats = { calls += "request" },
            stopOnRequestFailure = true,
            render = {
                calls += "render"
                0
            }
        )

        assertEquals(listOf("command"), calls)
        assertEquals(17, result.rc)
        assertFalse(result.requestAttempted)
        assertNull(result.renderRc)
    }

    @Test
    fun successfulCameraCommandBindsTheDeterminedFrameBeforeRender() {
        val calls = ArrayList<String>()
        var determinedFrame = 0L
        var requestedFrame = 0L

        val result = performSurfaceRenderTransaction(
            applyCommand = {
                calls += "command"
                determinedFrame = 42L
                0
            },
            requestCurrentStats = {
                calls += "request"
                requestedFrame = determinedFrame
            },
            stopOnRequestFailure = true,
            render = {
                calls += "render"
                assertEquals(determinedFrame, requestedFrame)
                0
            }
        )

        assertEquals(listOf("command", "request", "render"), calls)
        assertTrue(result.requestSucceeded)
        assertEquals(0, result.rc)
    }

    @Test
    fun strictRequestFailureDoesNotRenderTheUnobservedFrame() {
        val calls = ArrayList<String>()

        val result = performSurfaceRenderTransaction(
            applyCommand = {
                calls += "command"
                0
            },
            requestCurrentStats = {
                calls += "request"
                error("request failed")
            },
            stopOnRequestFailure = true,
            render = {
                calls += "render"
                0
            }
        )

        assertEquals(listOf("command", "request"), calls)
        assertTrue(result.requestAttempted)
        assertFalse(result.requestSucceeded)
        assertNull(result.renderRc)
    }
}
