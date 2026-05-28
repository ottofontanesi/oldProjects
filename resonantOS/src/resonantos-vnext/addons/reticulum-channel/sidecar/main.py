"""
Reticulum Channel Sidecar

Python sidecar process that runs the Reticulum networking stack via the `rns`
library, announces a destination, handles LXMF message encoding/decoding,
manages transport interfaces, and communicates with the host via stdio JSON-RPC.
"""

import sys
import json
import time
import os
import logging
from typing import Optional, Dict, List, Any
from dataclasses import dataclass, field
from datetime import datetime, timezone

try:
    import RNS
    import LXMF
except ImportError:
    # Allow module to load for testing without rns/lxmf installed
    RNS = None  # type: ignore
    LXMF = None  # type: ignore

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
    stream=sys.stderr,
)
logger = logging.getLogger("reticulum-sidecar")


@dataclass
class SidecarConfig:
    """Configuration for the Reticulum sidecar."""

    config_path: str = "~/.reticulum"
    identity_label: str = "ResonantOS"
    storage_path: str = "~/.reticulum/storage"


class ReticulumSidecar:
    """Main sidecar process managing the Reticulum stack and LXMF messaging."""

    # Maximum chunk size for message splitting (bytes)
    DEFAULT_MTU = 500

    def __init__(self, config: SidecarConfig):
        self.config = config
        self.reticulum: Optional[Any] = None
        self.identity: Optional[Any] = None
        self.destination: Optional[Any] = None
        self.lxmf_router: Optional[Any] = None
        self.active_links: Dict[str, Any] = {}
        self.pending_messages: Dict[str, dict] = {}
        self.running: bool = False
        self._destination_hash: Optional[str] = None
        self._active_interfaces: List[str] = []

    def start(self) -> dict:
        """
        Initialize Reticulum, create identity, announce destination.
        Returns: { destination_hash, active_interfaces }
        """
        if RNS is None:
            raise RuntimeError("RNS library not available. Install with: pip install rns")

        config_path = os.path.expanduser(self.config.config_path)
        storage_path = os.path.expanduser(self.config.storage_path)

        # Initialize Reticulum
        self.reticulum = RNS.Reticulum(config_path)

        # Create or load identity
        identity_path = os.path.join(config_path, "identity")
        if os.path.exists(identity_path):
            self.identity = RNS.Identity.from_file(identity_path)
            logger.info("Loaded existing identity")
        else:
            self.identity = RNS.Identity()
            logger.info("Created new identity")

        # Create destination
        self.destination = RNS.Destination(
            self.identity, RNS.Destination.IN, "resonantos", "messenger"
        )

        # Set up LXMF router
        self.lxmf_router = LXMF.LXMRouter(
            identity=self.identity, storagepath=storage_path
        )
        self.lxmf_router.register_delivery_callback(self._on_message_received)

        # Set display name
        self.destination.set_default_app_data(
            self.config.identity_label.encode("utf-8")
        )

        # Announce destination for peer discovery
        self.destination.announce()

        self._destination_hash = RNS.hexrep(self.destination.hash, delimit=False)
        self._active_interfaces = self._detect_interfaces()
        self.running = True

        logger.info(
            f"Sidecar started. Destination: {self._destination_hash}, "
            f"Interfaces: {self._active_interfaces}"
        )

        return {
            "destination_hash": self._destination_hash,
            "active_interfaces": self._active_interfaces,
        }

    def stop(self) -> dict:
        """Graceful shutdown: close links, stop Reticulum."""
        logger.info("Stopping sidecar...")
        self.running = False

        # Close active links
        for link_id, link in self.active_links.items():
            try:
                link.teardown()
            except Exception as e:
                logger.warning(f"Error closing link {link_id}: {e}")

        self.active_links.clear()
        return {"stopped": True}

    def send_message(
        self, destination_hash: str, content: str, priority: str = "normal"
    ) -> dict:
        """
        Encode as LXMF and transmit. Returns { message_id, queued }.
        Chunks if content exceeds transport MTU.
        """
        if not self.running:
            raise RuntimeError("Sidecar is not running")

        if LXMF is None:
            raise RuntimeError("LXMF library not available")

        # Resolve destination
        dest_hash = bytes.fromhex(destination_hash)
        dest_identity = RNS.Identity.recall(dest_hash)

        if dest_identity is None:
            # No known identity - queue for later
            message_id = f"queued-{int(time.time() * 1000)}"
            self.pending_messages[message_id] = {
                "destination_hash": destination_hash,
                "content": content,
                "priority": priority,
            }
            return {"message_id": message_id, "queued": True}

        # Create LXMF destination
        lxmf_dest = RNS.Destination(
            dest_identity, RNS.Destination.OUT, "resonantos", "messenger"
        )

        # Check if chunking is needed
        content_bytes = content.encode("utf-8")
        chunks = self._chunk_content(content_bytes, self.DEFAULT_MTU)

        message_id = None
        for i, chunk in enumerate(chunks):
            lxmf_message = LXMF.LXMessage(
                lxmf_dest,
                self.destination,
                chunk.decode("utf-8"),
                title="",
                desired_method=LXMF.LXMessage.DIRECT,
            )
            lxmf_message.register_delivery_callback(self._on_delivery_confirmed)

            self.lxmf_router.handle_outbound(lxmf_message)

            if i == 0:
                message_id = RNS.hexrep(lxmf_message.hash, delimit=False)

        logger.info(
            f"Sent message to {destination_hash} "
            f"({len(chunks)} chunk(s), id: {message_id})"
        )

        return {"message_id": message_id, "queued": False}

    def get_status(self) -> dict:
        """Return current state, interfaces, peer count, queue size."""
        state = "running" if self.running else "offline"
        return {
            "state": state,
            "destination_hash": self._destination_hash or "",
            "active_interfaces": [
                {"name": iface, "type": self._classify_interface(iface), "active": True, "error": None}
                for iface in self._active_interfaces
            ],
            "peers_count": len(self.active_links),
            "queued_messages": len(self.pending_messages),
        }

    def list_peers(self) -> dict:
        """Return known peers with last_seen and link status."""
        peers = []
        for dest_hash, link in self.active_links.items():
            peers.append(
                {
                    "destination_hash": dest_hash,
                    "display_name": None,
                    "last_seen": self._now_iso(),
                    "link_active": True,
                }
            )
        return {"peers": peers}

    def ping(self) -> dict:
        """Health check response."""
        return {"pong": True}

    def _on_message_received(self, message: Any) -> None:
        """Callback when LXMF message arrives. Emit notification to host."""
        try:
            # Extract text content only (ignore attachments)
            content = message.content_as_string() if hasattr(message, "content_as_string") else str(message.content)

            source_hash = RNS.hexrep(message.source_hash, delimit=False)
            source_name = None
            if hasattr(message, "source_name") and message.source_name:
                source_name = message.source_name

            notification = {
                "jsonrpc": "2.0",
                "method": "message_received",
                "params": {
                    "source_hash": source_hash,
                    "source_name": source_name,
                    "content": content,
                    "timestamp": self._now_iso(),
                    "lxmf_message_id": RNS.hexrep(message.hash, delimit=False),
                },
            }
            self._emit_notification(notification)
            logger.info(f"Received message from {source_hash}")
        except Exception as e:
            logger.error(f"Error processing inbound message: {e}")
            self._emit_error("inbound_processing_error", str(e))

    def _on_delivery_confirmed(self, message: Any) -> None:
        """Callback when delivery receipt arrives."""
        try:
            message_id = RNS.hexrep(message.hash, delimit=False)
            notification = {
                "jsonrpc": "2.0",
                "method": "delivery_confirmed",
                "params": {
                    "message_id": message_id,
                    "delivered_at": self._now_iso(),
                },
            }
            self._emit_notification(notification)
            logger.info(f"Delivery confirmed for {message_id}")
        except Exception as e:
            logger.error(f"Error processing delivery confirmation: {e}")

    def _emit_notification(self, notification: dict) -> None:
        """Write JSON-RPC notification to stdout."""
        sys.stdout.write(json.dumps(notification) + "\n")
        sys.stdout.flush()

    def _emit_error(self, code: str, message: str, details: Optional[str] = None) -> None:
        """Emit an error notification."""
        notification = {
            "jsonrpc": "2.0",
            "method": "error",
            "params": {
                "code": code,
                "message": message,
                "details": details,
            },
        }
        self._emit_notification(notification)

    def _process_stdin(self) -> None:
        """Read JSON-RPC requests from stdin, dispatch to handlers."""
        for line in sys.stdin:
            line = line.strip()
            if not line:
                continue

            try:
                request = json.loads(line)
            except json.JSONDecodeError as e:
                self._write_error_response(None, -32700, f"Parse error: {e}")
                continue

            method = request.get("method")
            params = request.get("params", {})
            request_id = request.get("id")

            handlers = {
                "start": lambda p: self.start(),
                "stop": lambda p: self.stop(),
                "send_message": lambda p: self.send_message(**p),
                "get_status": lambda p: self.get_status(),
                "list_peers": lambda p: self.list_peers(),
                "ping": lambda p: self.ping(),
            }

            handler = handlers.get(method)
            if handler:
                try:
                    result = handler(params)
                    response = {
                        "jsonrpc": "2.0",
                        "id": request_id,
                        "result": result,
                    }
                except Exception as e:
                    response = {
                        "jsonrpc": "2.0",
                        "id": request_id,
                        "error": {"code": -1, "message": str(e)},
                    }
            else:
                response = {
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "error": {
                        "code": -32601,
                        "message": f"Unknown method: {method}",
                    },
                }

            sys.stdout.write(json.dumps(response) + "\n")
            sys.stdout.flush()

    def _write_error_response(
        self, request_id: Optional[Any], code: int, message: str
    ) -> None:
        """Write a JSON-RPC error response."""
        response = {
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {"code": code, "message": message},
        }
        sys.stdout.write(json.dumps(response) + "\n")
        sys.stdout.flush()

    def _chunk_content(self, content_bytes: bytes, mtu: int) -> List[bytes]:
        """Split content into chunks that fit within the MTU."""
        if len(content_bytes) <= mtu:
            return [content_bytes]

        chunks = []
        for i in range(0, len(content_bytes), mtu):
            chunks.append(content_bytes[i : i + mtu])
        return chunks

    def _detect_interfaces(self) -> List[str]:
        """Auto-detect available transport interfaces from Reticulum config."""
        interfaces = []
        if self.reticulum is None:
            return interfaces

        try:
            for interface in RNS.Transport.interfaces:
                try:
                    interfaces.append(interface.name if hasattr(interface, "name") else str(interface))
                except Exception:
                    continue
        except Exception as e:
            logger.warning(f"Error detecting interfaces: {e}")

        return interfaces

    def _classify_interface(self, name: str) -> str:
        """Classify an interface name into a transport type."""
        name_lower = name.lower()
        if "tcp" in name_lower:
            return "tcp"
        elif "lora" in name_lower or "rnode" in name_lower:
            return "lora"
        elif "serial" in name_lower:
            return "serial"
        elif "i2p" in name_lower:
            return "i2p"
        else:
            return "auto"

    @staticmethod
    def _now_iso() -> str:
        """Return current UTC time as ISO-8601 string."""
        return datetime.now(timezone.utc).isoformat()


def main():
    """Entry point for the sidecar process."""
    config = SidecarConfig()

    # Parse config from environment or command-line args
    if len(sys.argv) > 1:
        config.config_path = sys.argv[1]
    if len(sys.argv) > 2:
        config.identity_label = sys.argv[2]

    config.config_path = os.environ.get("RETICULUM_CONFIG_PATH", config.config_path)
    config.identity_label = os.environ.get(
        "RETICULUM_IDENTITY_LABEL", config.identity_label
    )

    sidecar = ReticulumSidecar(config)

    logger.info("Reticulum sidecar starting, waiting for JSON-RPC commands on stdin...")
    sidecar._process_stdin()


if __name__ == "__main__":
    main()
