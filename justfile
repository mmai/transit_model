gtfs2netexfr:
  cargo install --path gtfs2netexfr
lint:
  git submodule update --init
  xmllint --noout --nonet --huge --schema ./tests/NeTEx/xsd/NeTEx_publication.xsd netexfile.xml
