"""
Unit tests for the Reticulum Channel Sidecar.

Tests LXMF encoding/decoding, JSON-RPC request/response round-trip,
and message chunking without requiring the rns/lxmf libraries.
"""

import json
import sys
import os
import unittest
from unittest.mock import patch, MagicMock
from io import StringIO
from datetime import datetime, timezone

# Add sidecar directory to path
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from main import ReticulumSidecar, SidecarConfig


class TestMessageChunking(unittest.TestCase):
    """Test message chunking logic."""

    def setUp(self):
        self.config = SidecarConfig(
            config_path="/tmp/test-reticulum",
            identity_label="TestNode",
            storage_path="/tmp/test-storage",
        )
        self.sidecar = ReticulumSidecar(self.config)

    def test_short_message_no_chunking(self):
        """Messages within MTU should not be chunked."""
        content = b"Hello, world!"
        chunks = self.sidecar._chunk_content(content, 500)
        self.assertEqual(len(chunks), 1)
        self.assertEqual(chunks[0], content)

    def test_exact_mtu_no_chunking(self):
        """Message exactly at MTU should not be chunked."""
        content = b"x" * 500
        chunks = self.sidecar._chunk_content(content, 500)
        self.assertEqual(len(chunks), 1)
        self.assertEqual(chunks[0], content)

    def test_message_exceeding_mtu_is_chunked(self):
        """Messages exceeding MTU should be split into chunks."""
        content = b"x" * 1200
        chunks = self.sidecar._chunk_content(content, 500)
        self.assertEqual(len(chunks), 3)
        self.assertEqual(len(chunks[0]), 500)
        self.assertEqual(len(chunks[1]), 500)
        self.assertEqual(len(chunks[2]), 200)

    def test_chunking_preserves_content(self):
        """Reassembling chunks should produce original content."""
        content = b"Hello, this is a test message that needs chunking!"
        chunks = self.sidecar._chunk_content(content, 10)
        reassembled = b"".join(chunks)
        self.assertEqual(reassembled, content)

    def test_empty_content(self):
        """Empty content should produce one empty chunk."""
        content = b""
        chunks = self.sidecar._chunk_content(content, 500)
        self.assertEqual(len(chunks), 1)
        self.assertEqual(chunks[0], b"")


class TestInterfaceClassification(unittest.TestCase):
    """Test transport interface classification."""

    def setUp(self):
        self.config = SidecarConfig()
        self.sidecar = ReticulumSidecar(self.config)

    def test_tcp_interface(self):
        self.assertEqual(self.sidecar._classify_interface("TCPInterface"), "tcp")

    def test_lora_interface(self):
        self.assertEqual(self.sidecar._classify_interface("RNodeInterface"), "lora")
        self.assertEqual(self.sidecar._classify_interface("LoRaInterface"), "lora")

    def test_serial_interface(self):
        self.assertEqual(self.sidecar._classify_interface("SerialInterface"), "serial")

    def test_i2p_interface(self):
        self.assertEqual(self.sidecar._classify_interface("I2PInterface"), "i2p")

    def test_auto_interface(self):
        self.assertEqual(self.sidecar._classify_interface("AutoInterface"), "auto")
        self.assertEqual(self.sidecar._classify_interface("UnknownInterface"), "auto")


class TestJsonRpcProcessing(unittest.TestCase):
    """Test JSON-RPC stdin processing."""

    def setUp(self):
        self.config = SidecarConfig()
        self.sidecar = ReticulumSidecar(self.config)

    def test_ping_request(self):
        """Ping request should return pong response."""
        request = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "ping", "params": {}})
        stdin = StringIO(request + "\n")
        stdout = StringIO()

        with patch("sys.stdin", stdin), patch("sys.stdout", stdout):
            self.sidecar._process_stdin()

        output = stdout.getvalue().strip()
        response = json.loads(output)
        self.assertEqual(response["jsonrpc"], "2.0")
        self.assertEqual(response["id"], 1)
        self.assertEqual(response["result"], {"pong": True})

    def test_get_status_when_not_running(self):
        """get_status should return offline state when not running."""
        request = json.dumps({"jsonrpc": "2.0", "id": 2, "method": "get_status", "params": {}})
        stdin = StringIO(request + "\n")
        stdout = StringIO()

        with patch("sys.stdin", stdin), patch("sys.stdout", stdout):
            self.sidecar._process_stdin()

        output = stdout.getvalue().strip()
        response = json.loads(output)
        self.assertEqual(response["result"]["state"], "offline")

    def test_unknown_method(self):
        """Unknown method should return error -32601."""
        request = json.dumps({"jsonrpc": "2.0", "id": 3, "method": "unknown_method", "params": {}})
        stdin = StringIO(request + "\n")
        stdout = StringIO()

        with patch("sys.stdin", stdin), patch("sys.stdout", stdout):
            self.sidecar._process_stdin()

        output = stdout.getvalue().strip()
        response = json.loads(output)
        self.assertEqual(response["error"]["code"], -32601)
        self.assertIn("unknown_method", response["error"]["message"])

    def test_invalid_json(self):
        """Invalid JSON should return parse error."""
        stdin = StringIO("not valid json\n")
        stdout = StringIO()

        with patch("sys.stdin", stdin), patch("sys.stdout", stdout):
            self.sidecar._process_stdin()

        output = stdout.getvalue().strip()
        response = json.loads(output)
        self.assertEqual(response["error"]["code"], -32700)

    def test_list_peers_empty(self):
        """list_peers should return empty list when no peers."""
        request = json.dumps({"jsonrpc": "2.0", "id": 4, "method": "list_peers", "params": {}})
        stdin = StringIO(request + "\n")
        stdout = StringIO()

        with patch("sys.stdin", stdin), patch("sys.stdout", stdout):
            self.sidecar._process_stdin()

        output = stdout.getvalue().strip()
        response = json.loads(output)
        self.assertEqual(response["result"]["peers"], [])

    def test_stop_request(self):
        """Stop request should return stopped: true."""
        request = json.dumps({"jsonrpc": "2.0", "id": 5, "method": "stop", "params": {}})
        stdin = StringIO(request + "\n")
        stdout = StringIO()

        with patch("sys.stdin", stdin), patch("sys.stdout", stdout):
            self.sidecar._process_stdin()

        output = stdout.getvalue().strip()
        response = json.loads(output)
        self.assertEqual(response["result"]["stopped"], True)

    def test_multiple_requests(self):
        """Multiple requests should each get a response."""
        requests = (
            json.dumps({"jsonrpc": "2.0", "id": 1, "method": "ping", "params": {}}) + "\n"
            + json.dumps({"jsonrpc": "2.0", "id": 2, "method": "get_status", "params": {}}) + "\n"
        )
        stdin = StringIO(requests)
        stdout = StringIO()

        with patch("sys.stdin", stdin), patch("sys.stdout", stdout):
            self.sidecar._process_stdin()

        lines = stdout.getvalue().strip().split("\n")
        self.assertEqual(len(lines), 2)

        resp1 = json.loads(lines[0])
        resp2 = json.loads(lines[1])
        self.assertEqual(resp1["id"], 1)
        self.assertEqual(resp2["id"], 2)


class TestNowIso(unittest.TestCase):
    """Test ISO timestamp generation."""

    def test_returns_valid_iso_format(self):
        result = ReticulumSidecar._now_iso()
        # Should be parseable as ISO-8601
        dt = datetime.fromisoformat(result)
        self.assertIsNotNone(dt)


if __name__ == "__main__":
    unittest.main()
