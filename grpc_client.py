import os
import grpc
from grpc_generated import notification_pb2, notification_pb2_grpc


GRPC_NOTIFICATIONS_TARGET = os.getenv("GRPC_NOTIFICATIONS_TARGET", "localhost:50051")


def send_notification(user_id: str, title: str, body: str, channel: str = "push", metadata: str = ""):
    """Simple synchronous client helper to send a notification over gRPC."""
    with grpc.insecure_channel(GRPC_NOTIFICATIONS_TARGET) as channel_conn:
        stub = notification_pb2_grpc.NotificationServiceStub(channel_conn)
        request = notification_pb2.NotificationRequest(
            user_id=user_id,
            title=title,
            body=body,
            channel=channel,
            metadata=metadata,
        )
        return stub.SendNotification(request)
