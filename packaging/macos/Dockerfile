FROM joseluisq/rust-linux-darwin-builder:latest

# Rust is already installed in this image. 
# Ensure the target is added (though usually default in this builder, being explicit is safe)
RUN rustup target add x86_64-apple-darwin

ENV PATH="/usr/local/osxcross/target/bin:$PATH"
ENV CC=/usr/local/osxcross/target/bin/o64-clang
ENV CXX=/usr/local/osxcross/target/bin/o64-clang++
ENV AR=/usr/local/osxcross/target/bin/x86_64-apple-darwin22.4-ar

# Explicitly set for the target to ensure cc-rs picks it up
ENV CC_x86_64_apple_darwin=/usr/local/osxcross/target/bin/o64-clang
ENV CXX_x86_64_apple_darwin=/usr/local/osxcross/target/bin/o64-clang++
ENV AR_x86_64_apple_darwin=/usr/local/osxcross/target/bin/x86_64-apple-darwin22.4-ar

ENV PKG_CONFIG_ALLOW_CROSS=1

WORKDIR /project
