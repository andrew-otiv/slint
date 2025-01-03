# Maintaining these patches

## Steps for generating the ashpd patch as an example:
curl https://static.crates.io/crates/ashpd/0.9.2/download > ashpd.tar.gz  
tar -xvf ashpd.tar.gz  
cd ashpd-0.9.2  
git init  
git add *  
git commit -m "initial commit"  
(hack on the code)  
git diff > ~/src/slint/examples/bazel_build/external_crate_builds/ashpd.patch  

