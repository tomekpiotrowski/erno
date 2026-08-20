import { NgModule, ModuleWithProviders } from '@angular/core';
import { CommonModule } from '@angular/common';

import { ErnoConfig } from './erno.config';
import { provideErno } from './erno.providers';
import { ErnoDevtoolsComponent } from './devtools/erno-devtools.component';

@NgModule({
  imports: [CommonModule],
  declarations: [ErnoDevtoolsComponent],
  exports: [ErnoDevtoolsComponent],
})
export class ErnoModule {
  static forRoot(config: ErnoConfig): ModuleWithProviders<ErnoModule> {
    return {
      ngModule: ErnoModule,
      providers: [provideErno(config)],
    };
  }
}
