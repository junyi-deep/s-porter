FROM alpine:3.22

RUN apk add --no-cache netcat-openbsd tinyproxy

COPY tinyproxy.conf /etc/tinyproxy/tinyproxy.conf

EXPOSE 8888

CMD ["tinyproxy", "-d", "-c", "/etc/tinyproxy/tinyproxy.conf"]
