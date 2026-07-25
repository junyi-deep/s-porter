FROM alpine:3.22

RUN apk add --no-cache netcat-openbsd socat tinyproxy

COPY tinyproxy.conf /etc/tinyproxy/tinyproxy.conf

EXPOSE 8888

CMD ["sh", "-c", "socat TCP-LISTEN:22,bind=127.0.0.1,reuseaddr,fork TCP:ssh:22 & exec tinyproxy -d -c /etc/tinyproxy/tinyproxy.conf"]
