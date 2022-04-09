FROM archlinux:base-devel as builder

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

WORKDIR /kvssink
COPY amazon-kinesis-video-streams-producer-sdk-cpp/CMake ./CMake
COPY amazon-kinesis-video-streams-producer-sdk-cpp/src ./src
COPY amazon-kinesis-video-streams-producer-sdk-cpp/samples ./samples
COPY amazon-kinesis-video-streams-producer-sdk-cpp/CMakeLists.txt .
COPY amazon-kinesis-video-streams-producer-sdk-cpp/.gitmodules .
RUN mkdir build && cd build && cmake -DBUILD_GSTREAMER_PLUGIN=ON -DBUILD_DEPENDENCIES=OFF .. && make -j

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src/
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

WORKDIR /app
COPY --from=builder /app/target/release/rtsp-to-kvs ./
COPY --from=builder /kvssink/build /kvssink/build

ENV GST_PLUGIN_PATH=/kvssink/build

ENTRYPOINT [ "./rtsp-to-kvs" ]