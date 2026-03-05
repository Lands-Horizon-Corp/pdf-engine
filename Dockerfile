# Use a slim Debian base for a small footprint
FROM debian:bookworm-slim

# 1. Install required system libraries for PrinceXML
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

# 2. Download and install PrinceXML Generic Linux version
RUN wget https://www.princexml.com/download/prince-16.2-linux-generic-x86_64.tar.gz \
    && tar -xzf prince-16.2-linux-generic-x86_64.tar.gz \
    && cd prince-16.2-linux-generic-x86_64 \
    && ./install.sh /usr \
    && cd .. && rm -rf prince-16.2-linux-generic-x86_64*

# 3. Set a working directory
WORKDIR /data

# 4. Set Prince as the entrypoint
ENTRYPOINT ["prince"]