"""Data Loader for the Unified RL Policy training pipeline.

Connects to experience_buffer.db and tool_call_tracker.db (read-only),
joins records by delegation_packet_id, validates episodes, and handles
missing traces with neutral 0.5 efficiency.
"""

import json
import sqlite3
from dataclasses import dataclass, field
from typing import Dict, List, Optional


@dataclass
class TrainingEpisode:
    """A combined record from ExperienceRecord + ToolCallTrace for RL training."""

    delegation_packet_id: str
    timestamp: str
    task_type: str
    workload_class: str
    task_description: str
    # Agent selection outcome
    selected_agent_id: str
    candidate_agent_ids: List[str]
    logician_score: float  # 0.0-1.0
    outcome_status: str  # "passed" | "failed" | "degraded"
    outcome_duration_ms: int
    # Cost data
    selected_agent_cost_tokens: int
    max_candidate_cost_tokens: int
    # Tool call trace (may be None if trace unavailable)
    efficiency_ratio: Optional[float]
    total_tool_calls: Optional[int]
    useful_tool_calls: Optional[int]
    redundant_tool_calls: Optional[int]
    pattern_count: Optional[int]
    tool_sequence_signature: Optional[List[str]]
    # Agent historical stats
    agent_quality_scores: Dict[str, float] = field(default_factory=dict)
    agent_speed_scores: Dict[str, float] = field(default_factory=dict)
    agent_cost_scores: Dict[str, float] = field(default_factory=dict)
    agent_availability: Dict[str, float] = field(default_factory=dict)
    agent_efficiency_ratios: Dict[str, float] = field(default_factory=dict)


class DataLoader:
    """Loads and joins ExperienceRecords with ToolCallTraces into TrainingEpisodes."""

    def __init__(self, experience_db_path: str, tracker_db_path: str):
        self.experience_db = sqlite3.connect(
            f"file:{experience_db_path}?mode=ro", uri=True
        )
        self.experience_db.row_factory = sqlite3.Row
        self.tracker_db = sqlite3.connect(
            f"file:{tracker_db_path}?mode=ro", uri=True
        )
        self.tracker_db.row_factory = sqlite3.Row

    def load_episodes(
        self, since_timestamp: Optional[str] = None
    ) -> List[TrainingEpisode]:
        """Load all valid training episodes, joining experience records with tool traces."""
        cursor = self.experience_db.cursor()

        if since_timestamp:
            cursor.execute(
                """SELECT * FROM experience_records
                   WHERE timestamp >= ?
                   ORDER BY timestamp ASC""",
                (since_timestamp,),
            )
        else:
            cursor.execute(
                "SELECT * FROM experience_records ORDER BY timestamp ASC"
            )

        rows = cursor.fetchall()
        episodes: List[TrainingEpisode] = []

        for row in rows:
            episode = self._build_episode(row)
            if episode and self.validate_episode(episode):
                episodes.append(episode)

        return episodes

    def _build_episode(self, row: sqlite3.Row) -> Optional[TrainingEpisode]:
        """Build a TrainingEpisode from an experience record row, joining with tool trace."""
        try:
            delegation_packet_id = row["delegation_packet_id"]

            # Look up tool call trace
            trace = self._get_tool_trace(delegation_packet_id)

            # Parse JSON fields
            candidate_ids = json.loads(row["candidate_agent_ids_json"])
            agent_quality = json.loads(row["agent_quality_scores_json"]) if row["agent_quality_scores_json"] else {}
            agent_speed = json.loads(row["agent_speed_scores_json"]) if row["agent_speed_scores_json"] else {}
            agent_cost = json.loads(row["agent_cost_scores_json"]) if row["agent_cost_scores_json"] else {}
            agent_avail = json.loads(row["agent_availability_json"]) if row["agent_availability_json"] else {}
            agent_eff = json.loads(row["agent_efficiency_ratios_json"]) if row["agent_efficiency_ratios_json"] else {}

            # Handle missing trace with neutral 0.5 efficiency
            efficiency_ratio = trace["efficiency_ratio"] if trace else 0.5
            total_tool_calls = trace["total_tool_calls"] if trace else None
            useful_tool_calls = trace["useful_tool_calls"] if trace else None
            redundant_tool_calls = trace["redundant_tool_calls"] if trace else None
            pattern_count = trace["pattern_count"] if trace else None
            tool_sequence = (
                json.loads(trace["tool_sequence_signature_json"])
                if trace and trace["tool_sequence_signature_json"]
                else None
            )

            return TrainingEpisode(
                delegation_packet_id=delegation_packet_id,
                timestamp=row["timestamp"],
                task_type=row["task_type"],
                workload_class=row["workload_class"],
                task_description=row["task_description"],
                selected_agent_id=row["selected_agent_id"],
                candidate_agent_ids=candidate_ids,
                logician_score=float(row["logician_score"]),
                outcome_status=row["outcome_status"],
                outcome_duration_ms=int(row["outcome_duration_ms"]),
                selected_agent_cost_tokens=int(row["selected_agent_cost_tokens"]),
                max_candidate_cost_tokens=int(row["max_candidate_cost_tokens"]),
                efficiency_ratio=efficiency_ratio,
                total_tool_calls=total_tool_calls,
                useful_tool_calls=useful_tool_calls,
                redundant_tool_calls=redundant_tool_calls,
                pattern_count=pattern_count,
                tool_sequence_signature=tool_sequence,
                agent_quality_scores=agent_quality,
                agent_speed_scores=agent_speed,
                agent_cost_scores=agent_cost,
                agent_availability=agent_avail,
                agent_efficiency_ratios=agent_eff,
            )
        except (KeyError, TypeError, json.JSONDecodeError):
            return None

    def _get_tool_trace(self, delegation_packet_id: str) -> Optional[sqlite3.Row]:
        """Look up tool call trace by delegation_packet_id."""
        cursor = self.tracker_db.cursor()
        cursor.execute(
            """SELECT * FROM tool_call_traces
               WHERE delegation_packet_id = ?
               LIMIT 1""",
            (delegation_packet_id,),
        )
        return cursor.fetchone()

    def validate_episode(self, episode: TrainingEpisode) -> bool:
        """Validate required fields are present and within expected ranges."""
        if not episode.delegation_packet_id:
            return False
        if not episode.timestamp:
            return False
        if episode.logician_score is None:
            return False
        if not (0.0 <= episode.logician_score <= 1.0):
            return False
        if episode.outcome_status not in ("passed", "failed", "degraded"):
            return False
        if not episode.selected_agent_id:
            return False
        if not episode.candidate_agent_ids:
            return False
        return True

    def count_available_episodes(self) -> int:
        """Count total experience records available for training."""
        cursor = self.experience_db.cursor()
        cursor.execute("SELECT COUNT(*) FROM experience_records")
        result = cursor.fetchone()
        return result[0] if result else 0

    def close(self):
        """Close database connections."""
        self.experience_db.close()
        self.tracker_db.close()
