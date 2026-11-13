import { createApp } from 'vue';
import App from './App.vue';
import { bootstrapApp } from './app/bootstrap';
import './styles/app.css';

const app = createApp(App);

/** 邮件正文等动态内容排除自动 i18n（编译期插件未接入前亦作标记） */
app.directive('no-i18n', {});

bootstrapApp(app);
app.mount('#app');
