# Image published by the Release workflow, from the static binaries the CI built
# (`bin/<arch>/twitchminer`, see .github/workflows/release.yml). To build from the
# sources instead, use the Dockerfile at the root of the repository.
FROM alpine:3
ARG TARGETARCH
RUN apk add --no-cache ca-certificates tzdata && adduser -D -u 1000 miner && mkdir /data && chown miner /data
COPY bin/${TARGETARCH}/twitchminer /usr/local/bin/twitchminer
USER miner
ENV TM_DATA_DIR=/data TM_API_BIND=0.0.0.0:7878
VOLUME /data
EXPOSE 7878
ENTRYPOINT ["twitchminer"]
CMD ["run"]
