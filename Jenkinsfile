pipeline {
  agent any

  stages {
    stage('Build + Test + Coverage') {
      environment {
         RUST_BACKTRACE = 1
         RUST_LOG = palace_server=TRACE
      }
      steps {
         sh 'cargo tarpaulin -v -l --count -p palace_server --ignore-tests'
      }
    }
  }
}
