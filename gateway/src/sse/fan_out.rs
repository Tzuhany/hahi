// Fan-out: one conversation's Redis Stream events → multiple SSE connections.
// Handles the case where the same user has web + mobile open simultaneously.
