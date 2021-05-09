import './style.scss';
import React from 'react';
import { render } from '@instruments/common/index';
import { RootRadioPanel } from './Components/BaseRadioPanels';

render(
    <div className="rmp-wrapper">
        <RootRadioPanel side="L" />
        <RootRadioPanel side="R" />
    </div>,
);
