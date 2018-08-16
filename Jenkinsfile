pipeline {
  agent any

  stages {
    stage('Build + Test + Coverage') {
      steps {
         sh 'cargo tarpaulin -v -l --count -p palace_server --ignore-tests'
      }
    }
  }
}
