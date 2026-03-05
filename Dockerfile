
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    wget \
    ca-certificates \
    libfontconfig1 \
    libpixman-1-0 \
    libxml2 \
    libcurl4 \
    libgif7 \
    libjpeg62-turbo \
    libpng16-16 \
    libtiff6 \
    fonts-liberation \
    && rm -rf /var/lib/apt/lists/*

RUN wget https://www.princexml.com/download/prince-16.2-linux-generic-x86_64.tar.gz \
    && tar -xzf prince-16.2-linux-generic-x86_64.tar.gz \
    && cd prince-16.2-linux-generic-x86_64 \
    && ./install.sh /usr \
    && cd .. && rm -rf prince-16.2-linux-generic-x86_64*

WORKDIR /data

ENTRYPOINT ["prince"]