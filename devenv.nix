{ pkgs, lib, config, inputs, ... }:

{
  languages.rust.enable = true;
  env.LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
  packages = [ 
    # build requirements
    pkgs.proj pkgs.clang
    # optional
    #   libxml2 : provides xmllint to validate generated xml files against NeTEx XSD specs
    pkgs.libxml2
  ];
}
