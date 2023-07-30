#======================== builder
FROM rust:latest as builder
WORKDIR /usr/src/cut-optimizer-1d-server
COPY . .

## Install target platform (Cross-Compilation) --> Needed for Alpine
RUN rustup target add x86_64-unknown-linux-musl
ENV RUSTFLAGS='-C linker=x86_64-linux-gnu-gcc'

RUN cargo build --target x86_64-unknown-linux-musl --release
# RUN cargo install . --release


# ======================= production
FROM alpine:latest
ARG APP=/usr/src/app
WORKDIR ${APP}

ENV TZ=TEH/UTC \
  APP_USER=appuser

RUN addgroup -S $APP_USER \
  && adduser -S -g $APP_USER $APP_USER

RUN apk update \
  && apk add --no-cache ca-certificates tzdata postgresql-client bash openssl libgcc libstdc++ ncurses-libs libc6-compat gcompat\
  && rm -rf /var/cache/apk/* 

# COPY --from=builder /usr/local/cargo/bin/cut-optimizer-1d-server /usr/local/bin/cut-optimizer-1d-server
COPY --from=builder /usr/src/cut-optimizer-1d-server/target/release/ /usr/local/bin/
RUN ls /usr/local/bin/
RUN chown -R $APP_USER:$APP_USER ${APP}
USER $APP_USER

EXPOSE 3030
CMD [ "cut-optimizer-1d-server", "-vv" ]