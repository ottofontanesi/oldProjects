"""State Encoder for the Unified RL Policy training pipeline.

Handles sentence transformer loading (all-MiniLM-L6-v2), TF-IDF+PCA fallback
(64-dim), running normalization stats, and state vector construction for both
policy levels.
"""

import numpy as np
from dataclasses import dataclass
from typing import List, Optional


@dataclass
class StateEncoderConfig:
    """Configuration for the state encoder."""

    use_sentence_transformer: bool = True
    transformer_model: str = "all-MiniLM-L6-v2"
    transformer_dim: int = 384
    tfidf_pca_dim: int = 64
    agent_stats_dim: int = 5  # quality, speed, cost, availability, percentile
    tool_history_dim: int = 4  # avg_efficiency, pattern_rate, avg_calls, cost_per_call
    max_candidates: int = 10


class StateEncoder:
    """Encodes task descriptions and agent statistics into fixed-size state vectors."""

    def __init__(self, config: StateEncoderConfig):
        self.config = config
        self._transformer = None
        self._tfidf = None
        self._pca = None
        self._running_mean: Optional[np.ndarray] = None
        self._running_var: Optional[np.ndarray] = None
        self._sample_count: int = 0

    def _load_transformer(self):
        """Lazy-load the sentence transformer model."""
        if self._transformer is None:
            try:
                from sentence_transformers import SentenceTransformer

                self._transformer = SentenceTransformer(self.config.transformer_model)
            except (ImportError, OSError):
                # Fall back to TF-IDF+PCA if transformer unavailable
                self._transformer = None
                self.config.use_sentence_transformer = False

    def encode_task(self, task_description: str) -> np.ndarray:
        """Encode task description to embedding vector (384-dim or 64-dim fallback)."""
        if self.config.use_sentence_transformer:
            self._load_transformer()
            if self._transformer is not None:
                embedding = self._transformer.encode(
                    task_description, convert_to_numpy=True
                )
                return embedding.astype(np.float32).flatten()

        # TF-IDF + PCA fallback
        if self._tfidf is not None and self._pca is not None:
            tfidf_vec = self._tfidf.transform([task_description])
            pca_vec = self._pca.transform(tfidf_vec.toarray())
            return pca_vec.astype(np.float32).flatten()

        # Ultimate fallback: zero vector
        dim = (
            self.config.transformer_dim
            if self.config.use_sentence_transformer
            else self.config.tfidf_pca_dim
        )
        return np.zeros(dim, dtype=np.float32)

    def encode_agent_stats(
        self,
        quality: float,
        speed_ms: float,
        cost_tokens: float,
        availability: float,
        percentile: float,
    ) -> np.ndarray:
        """Encode agent statistics to fixed 5-dim vector, all normalized to [0,1]."""
        # Normalize speed: assume max 60000ms, clamp to [0,1]
        speed_norm = min(1.0, max(0.0, speed_ms / 60000.0))
        # Normalize cost: assume max 100000 tokens, clamp to [0,1]
        cost_norm = min(1.0, max(0.0, cost_tokens / 100000.0))
        # Quality, availability, percentile already in [0,1]
        quality_norm = min(1.0, max(0.0, quality))
        avail_norm = min(1.0, max(0.0, availability))
        pct_norm = min(1.0, max(0.0, percentile))

        return np.array(
            [quality_norm, speed_norm, cost_norm, avail_norm, pct_norm],
            dtype=np.float32,
        )

    def encode_tool_history(
        self,
        avg_efficiency: float,
        pattern_rate: float,
        avg_calls: float,
        cost_per_call: float,
    ) -> np.ndarray:
        """Encode tool usage history to fixed 4-dim vector."""
        # Normalize: efficiency in [0,1], pattern_rate per 100, avg_calls / 50, cost / 1000
        eff_norm = min(1.0, max(0.0, avg_efficiency))
        pattern_norm = min(1.0, max(0.0, pattern_rate / 100.0))
        calls_norm = min(1.0, max(0.0, avg_calls / 50.0))
        cost_norm = min(1.0, max(0.0, cost_per_call / 1000.0))

        return np.array(
            [eff_norm, pattern_norm, calls_norm, cost_norm], dtype=np.float32
        )

    def build_high_level_state(
        self,
        task_embedding: np.ndarray,
        agent_stats: List[np.ndarray],
        tool_histories: List[np.ndarray],
        low_level_efficiency_estimate: float,
    ) -> np.ndarray:
        """Concatenate all features into the high-level policy state vector.

        Layout: [task_embedding | agent_stats_0 | tool_hist_0 | ... | agent_stats_N | tool_hist_N | efficiency_estimate]
        Pads with zeros if fewer than max_candidates agents.
        """
        per_agent_dim = self.config.agent_stats_dim + self.config.tool_history_dim
        max_agents = self.config.max_candidates

        # Pad or truncate agent features
        agent_features = np.zeros(per_agent_dim * max_agents, dtype=np.float32)
        num_agents = min(len(agent_stats), max_agents)

        for i in range(num_agents):
            offset = i * per_agent_dim
            stats = agent_stats[i] if i < len(agent_stats) else np.zeros(self.config.agent_stats_dim, dtype=np.float32)
            hist = tool_histories[i] if i < len(tool_histories) else np.zeros(self.config.tool_history_dim, dtype=np.float32)
            agent_features[offset : offset + self.config.agent_stats_dim] = stats
            agent_features[offset + self.config.agent_stats_dim : offset + per_agent_dim] = hist

        efficiency_arr = np.array([low_level_efficiency_estimate], dtype=np.float32)
        state = np.concatenate([task_embedding, agent_features, efficiency_arr])
        return state

    def build_low_level_state(
        self,
        task_embedding: np.ndarray,
        tool_sequence_so_far: List[str],
        selected_agent_tool_history: np.ndarray,
    ) -> np.ndarray:
        """Build the low-level policy state vector.

        Layout: [task_embedding | tool_sequence_encoding (64-dim) | agent_tool_history]
        """
        # Encode tool sequence as a simple bag-of-tools hash to 64 dimensions
        tool_seq_encoding = np.zeros(64, dtype=np.float32)
        for i, tool in enumerate(tool_sequence_so_far[:64]):
            # Simple hash-based encoding
            hash_val = hash(tool) % 64
            tool_seq_encoding[hash_val] += 1.0 / (i + 1)

        # Normalize tool sequence encoding
        norm = np.linalg.norm(tool_seq_encoding)
        if norm > 0:
            tool_seq_encoding = tool_seq_encoding / norm

        state = np.concatenate(
            [task_embedding, tool_seq_encoding, selected_agent_tool_history]
        )
        return state

    def normalize(self, state: np.ndarray) -> np.ndarray:
        """Apply running z-score normalization."""
        if self._running_mean is None or self._running_var is None:
            return state

        # Avoid division by zero
        std = np.sqrt(self._running_var + 1e-8)
        return ((state - self._running_mean) / std).astype(np.float32)

    def update_running_stats(self, batch: np.ndarray):
        """Update running mean and variance from a training batch (Welford's method)."""
        batch_mean = np.mean(batch, axis=0)
        batch_var = np.var(batch, axis=0)
        batch_count = batch.shape[0]

        if self._running_mean is None:
            self._running_mean = batch_mean
            self._running_var = batch_var
            self._sample_count = batch_count
        else:
            total_count = self._sample_count + batch_count
            delta = batch_mean - self._running_mean
            new_mean = self._running_mean + delta * (batch_count / total_count)
            m_a = self._running_var * self._sample_count
            m_b = batch_var * batch_count
            m2 = m_a + m_b + (delta**2) * self._sample_count * batch_count / total_count
            new_var = m2 / total_count

            self._running_mean = new_mean
            self._running_var = new_var
            self._sample_count = total_count

    def fit_tfidf_pca(self, corpus: List[str]):
        """Fit TF-IDF + PCA fallback from existing experience buffer corpus."""
        from sklearn.decomposition import PCA
        from sklearn.feature_extraction.text import TfidfVectorizer

        self._tfidf = TfidfVectorizer(max_features=1000)
        tfidf_matrix = self._tfidf.fit_transform(corpus)

        n_components = min(self.config.tfidf_pca_dim, tfidf_matrix.shape[1], tfidf_matrix.shape[0])
        self._pca = PCA(n_components=n_components)
        self._pca.fit(tfidf_matrix.toarray())

    @property
    def high_level_state_dim(self) -> int:
        """Total dimension of the high-level state vector."""
        task_dim = (
            self.config.transformer_dim
            if self.config.use_sentence_transformer
            else self.config.tfidf_pca_dim
        )
        per_agent = self.config.agent_stats_dim + self.config.tool_history_dim
        return task_dim + (per_agent * self.config.max_candidates) + 1  # +1 for low-level estimate

    @property
    def low_level_state_dim(self) -> int:
        """Total dimension of the low-level state vector."""
        task_dim = (
            self.config.transformer_dim
            if self.config.use_sentence_transformer
            else self.config.tfidf_pca_dim
        )
        return task_dim + 64 + self.config.tool_history_dim  # 64 for tool sequence encoding
