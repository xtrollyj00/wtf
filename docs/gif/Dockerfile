# Records docs/demo.gif with this checkout's wtf (docs/gif/demo.py says how), from the
# repository root:
#   docker build -f docs/gif/Dockerfile -t wtf-gif .
#   docker run --rm -v "$PWD":/work -u "$(id -u):$(id -g)" wtf-gif
FROM rust:1-trixie AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked

FROM debian:trixie-slim
# a browser to draw each frame, Noto Sans Mono for docs/screenshot.py, ffmpeg to join them,
# pyte to replay a recording for docs/agent/cast2gif.py
RUN apt-get update && apt-get install -y --no-install-recommends chromium ffmpeg python3 python3-pyte fonts-noto-core \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/wtf /usr/local/bin/wtf
WORKDIR /work
ENV HOME=/tmp
CMD ["python3", "docs/gif/demo.py"]
