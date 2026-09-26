FROM archlinux:base
WORKDIR /app
COPY space-server /app/space-server
COPY web/ /app/web/dist/
ENV SPACE_ROOT=/app SPACE_BIND=0.0.0.0:4321
USER 1000:1000
EXPOSE 4321
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD ["bash", "-c", "exec 3<>/dev/tcp/127.0.0.1/4321; printf 'GET /health HTTP/1.0\r\nHost: localhost\r\n\r\n' >&3; read -r status <&3; [[ $status == *'200 OK'* ]]"]
CMD ["/app/space-server"]
