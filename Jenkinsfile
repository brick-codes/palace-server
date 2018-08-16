pipeline {
  agent any

  stages {
    stage('Build + Test + Coverage') {
      steps {
         sh 'cargo tarpaulin -v --count -p palace_server --exclude-files "tests/*"'
      }
    }
  }
}
