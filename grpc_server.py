import logging
from concurrent import futures
import grpc

from grpc_generated import notification_pb2, notification_pb2_grpc

logger = logging.getLogger(__name__)


class NotificationService(notification_pb2_grpc.NotificationServiceServicer):
    def SendNotification(self, request, context):
        # TODO: integrate with real notification channels (push/email/SMS)
        logger.info(
            "Sending notification",
            extra={
                "user_id": request.user_id,
                "channel": request.channel,
                "title": request.title,
            },
        )
        return notification_pb2.NotificationResponse(success=True, error="")


def serve(port: int = 50051) -> None:
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=10))
    notification_pb2_grpc.add_NotificationServiceServicer_to_server(
        NotificationService(), server
    )
    server.add_insecure_port(f"[::]:{port}")
    logger.info("Starting gRPC NotificationService on port %s", port)
    server.start()
    server.wait_for_termination()


if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO)
    serve()
