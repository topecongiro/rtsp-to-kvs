FROM archlinux:base-devel as builder-base

RUN pacman -Syu --noconfirm
RUN pacman -Syu --noconfirm \
    base-devel \
    cmake \
    git \
    gstreamer \
    gst-libav \
    gst-plugins-bad \
    gst-plugins-base \
    gst-plugins-good \
    log4cplus \
    rust

FROM builder-base as kvssink-builder

WORKDIR /kvssink
COPY amazon-kinesis-video-streams-producer-sdk-cpp/CMake ./CMake
COPY amazon-kinesis-video-streams-producer-sdk-cpp/src ./src
COPY amazon-kinesis-video-streams-producer-sdk-cpp/samples ./samples
COPY amazon-kinesis-video-streams-producer-sdk-cpp/CMakeLists.txt .
COPY amazon-kinesis-video-streams-producer-sdk-cpp/.gitmodules .
RUN mkdir build && cd build && cmake -DBUILD_GSTREAMER_PLUGIN=ON -DBUILD_DEPENDENCIES=OFF .. && make -j

FROM builder-base as app-builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
RUN mkdir ./src && echo 'fn main() {}' > ./src/main.rs
RUN cargo build --release && rm -rf ./src
COPY src ./src 
RUN touch -a -m ./src/main.rs
RUN cargo build --release

FROM archlinux:base-devel

RUN pacman -Syu --noconfirm
RUN pacman -Syu --noconfirm \
    log4cplus \
    openssl \
    gstreamer \
    gst-plugins-base \
    gst-plugins-good \
    gst-plugins-bad \
    gst-libav

COPY --from=app-builder /app/target/release/rtsp-to-kvs /app/rtsp-to-kvs
COPY --from=kvssink-builder /kvssink/build /kvssink/build

ENV GST_PLUGIN_PATH=/kvssink/build

ENTRYPOINT [ "/app/rtsp-to-kvs" ]
