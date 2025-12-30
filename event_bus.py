import logging

logger = logging.getLogger(__name__)

def emit_event(event_type: str, topic: str, payload: dict):
    """Simple no-op event emitter used during local dev."""
    logger.info(f"[event_bus] {event_type} -> {topic}: {payload}")
